// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::args::VerificationArgs;
use crate::call_single_file::LibConfig;
use crate::project::Artifact;
use crate::session::{
    KaniSession, get_cargo_path, lib_folder, lib_no_core_folder, setup_cargo_command,
    setup_cargo_command_inner,
};
use crate::util;
use crate::util::args::{CargoArg, CommandWrapper as _, KaniArg, PassTo, encode_as_rustc_arg};
use anyhow::{Context, Result, bail};
use cargo_metadata::diagnostic::{Diagnostic, DiagnosticLevel};
use cargo_metadata::{
    Artifact as RustcArtifact, CrateType, Message, Metadata, MetadataCommand, Package, PackageId,
    Target, TargetKind,
};
use kani_metadata::{ArtifactType, CompilerArtifactStub};
use std::collections::HashMap;
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io::IsTerminal;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, trace};

/// The outputs of kani-compiler being invoked via cargo on a project.
pub struct CargoOutputs {
    /// The directory where compiler outputs should be directed.
    /// Usually 'target/BUILD_TRIPLE/debug/deps/'
    pub outdir: PathBuf,
    /// The kani-metadata.json files written by kani-compiler.
    pub metadata: Vec<Artifact>,
    /// Recording the cargo metadata from the build
    pub cargo_metadata: Metadata,
}

impl KaniSession {
    /// Create a new cargo library in the given path.
    ///
    /// Since we cannot create a new workspace with `cargo init --lib`, we create the dummy
    /// crate manually. =( See <https://github.com/rust-lang/cargo/issues/8365>.
    ///
    /// Without setting up a new workspace, cargo init will modify the workspace where this is
    /// running. See <https://github.com/model-checking/kani/issues/3574> for details.
    pub fn cargo_init_lib(&self, path: &Path) -> Result<()> {
        let toml_path = path.join("Cargo.toml");
        if toml_path.exists() {
            bail!("Cargo.toml already exists in {}", path.display());
        }

        // Create folder for library
        fs::create_dir_all(path.join("src"))?;

        // Create dummy crate and write dummy body
        let lib_path = path.join("src/lib.rs");
        fs::write(&lib_path, "pub fn dummy() {}")?;

        // Create Cargo.toml
        fs::write(
            &toml_path,
            r#"[package]
name = "dummy"
version = "0.1.0"

[lib]
crate-type = ["lib"]

[workspace]
"#,
        )?;
        Ok(())
    }

    pub fn cargo_build_std(&self, std_path: &Path, krate_path: &Path) -> Result<Vec<Artifact>> {
        let lib_path = lib_no_core_folder().unwrap();
        let mut rustc_args = self.kani_rustc_flags(LibConfig::new_no_core(lib_path));

        // In theory, these could be passed just to the local crate rather than all crates,
        // but the `cargo build` command we use for building `std` doesn't allow you to pass `rustc`
        // arguments, so we have to pass them through the environment variable instead.
        rustc_args.push(encode_as_rustc_arg(&self.kani_compiler_local_flags()));

        // Ignore global assembly, since `compiler_builtins` has some.
        rustc_args.push(encode_as_rustc_arg(&[
            KaniArg::from("--ignore-global-asm"),
            self.reachability_arg(),
        ]));

        let mut cargo_args: Vec<CargoArg> = vec!["build".into()];
        cargo_args.append(&mut cargo_config_args());

        // Configuration needed to parse cargo compilation status.
        cargo_args.push("--message-format".into());
        cargo_args.push("json-diagnostic-rendered-ansi".into());
        cargo_args.push("-Z".into());
        cargo_args.push("build-std=panic_abort,core,std".into());

        if self.args.common_args.verbose {
            cargo_args.push("-v".into());
        }

        // We need this suffix push because of https://github.com/rust-lang/cargo/pull/14370
        // which removes the library suffix from the build-std command
        let mut full_path = std_path.to_path_buf();
        full_path.push("library");

        // Since we are verifying the standard library, we set the reachability to all crates.
        let mut cmd = setup_cargo_command()?;
        cmd.pass_cargo_args(&cargo_args)
            .current_dir(krate_path)
            .env("RUSTC", &self.kani_compiler)
            .pass_rustc_args(&rustc_args, PassTo::AllCrates)
            .env("CARGO_TERM_PROGRESS_WHEN", "never")
            .env("__CARGO_TESTS_ONLY_SRC_ROOT", full_path.as_os_str());

        Ok(self
            .run_build(cmd)?
            .into_iter()
            .filter_map(|artifact| {
                if artifact.target.crate_types.contains(&CrateType::Lib)
                    || artifact.target.crate_types.contains(&CrateType::RLib)
                {
                    map_kani_artifact(artifact)
                } else {
                    None
                }
            })
            .collect())
    }

    /// Calls `cargo_build` to generate `*.symtab.json` files in `target_dir`
    pub fn cargo_build(&mut self, keep_going: bool) -> Result<CargoOutputs> {
        let build_target = env!("TARGET"); // see build.rs
        let metadata = self.cargo_metadata(build_target)?;
        let target_dir = self
            .args
            .target_dir
            .as_ref()
            .unwrap_or(&metadata.target_directory.clone().into())
            .clone()
            .join("kani");
        let outdir = target_dir.join(build_target).join("debug/deps");

        if self.args.force_build && target_dir.exists() {
            fs::remove_dir_all(&target_dir)?;
        }

        let lib_path = lib_folder().unwrap();
        let mut rustc_args = self.kani_rustc_flags(LibConfig::new(lib_path));
        rustc_args.push(encode_as_rustc_arg(&self.kani_compiler_dependency_flags()));

        let mut cargo_args: Vec<CargoArg> = vec!["rustc".into()];
        if let Some(path) = &self.args.cargo.manifest_path {
            cargo_args.push("--manifest-path".into());
            cargo_args.push(path.into());
        }
        if self.args.cargo.all_features {
            cargo_args.push("--all-features".into());
        }
        if self.args.cargo.no_default_features {
            cargo_args.push("--no-default-features".into());
        }
        let features = self.args.cargo.features();
        if !features.is_empty() {
            cargo_args.push(format!("--features={}", features.join(",")).into());
        }

        cargo_args.append(&mut cargo_config_args());

        cargo_args.push("--target-dir".into());
        cargo_args.push(target_dir.into());

        // Configuration needed to parse cargo compilation status.
        cargo_args.push("--message-format".into());
        cargo_args.push("json-diagnostic-rendered-ansi".into());

        if self.args.tests {
            // Use test profile in order to pull dev-dependencies and compile using `--test`.
            // Initially the plan was to use `--tests` but that brings in multiple targets.
            cargo_args.push("--profile".into());
            cargo_args.push("test".into());
        }

        if self.args.common_args.verbose {
            cargo_args.push("-v".into());
        }

        // Arguments that will only be passed to the target package (the package under verification)
        // and not its dependencies, c.f. https://doc.rust-lang.org/cargo/commands/cargo-rustc.html.
        // The difference between pkg_args and rustc_args is that rustc_args are also provided when
        // we invoke rustc on the target package's dependencies.
        // We do not provide the `--reachability` argument to dependencies so that it has the default value `None`
        // (c.f. kani-compiler::args::ReachabilityType) and we skip codegen for the dependency.
        // This is the desired behavior because we only want to construct `CodegenUnits` for the target package;
        // i.e., if some dependency has harnesses, we don't want to run them.

        // If you are adding a new `kani-compiler` argument, you likely want to put it here, unless there is a specific
        // reason it would be used in dependencies that are skipping reachability and codegen.
        // Note that passing compiler args to dependencies is a currently no-op, since `--reachability=None` skips codegen
        // anyway. However, this will cause unneeded recompilation of dependencies should those args change, and thus
        // should be avoided if possible.
        let mut kani_pkg_args = vec![self.reachability_arg()];
        kani_pkg_args.extend(self.kani_compiler_local_flags());

        let mut found_target = false;
        let packages = self.packages_to_verify(&self.args, &metadata)?;
        let mut artifacts = vec![];
        let mut failed_targets = vec![];
        for package in packages {
            for verification_target in package_targets(&self.args, package) {
                let mut cmd =
                    setup_cargo_command_inner(Some(verification_target.target().name.clone()))?;
                cmd.pass_cargo_args(&cargo_args)
                    .args(vec!["-p", &package.id.to_string()])
                    .args(verification_target.to_args())
                    .arg("--") // Add this delimiter so we start passing args to rustc and not Cargo
                    .env("RUSTC", &self.kani_compiler)
                    .pass_rustc_args(&rustc_args, PassTo::AllCrates)
                    .pass_rustc_arg(encode_as_rustc_arg(&kani_pkg_args), PassTo::OnlyLocalCrate)
                    // This is only required for stable but is a no-op for nightly channels
                    .env("RUSTC_BOOTSTRAP", "1")
                    .env("CARGO_TERM_PROGRESS_WHEN", "never");

                match self.run_build_target(cmd, verification_target.target()) {
                    Err(err) => {
                        if keep_going {
                            let target_str = format!("{verification_target}");
                            util::error(&format!("Failed to compile {target_str}"));
                            failed_targets.push(target_str);
                        } else {
                            return Err(err);
                        }
                    }
                    Ok(Some(artifact)) => artifacts.push(artifact),
                    Ok(None) => {}
                }
                found_target = true;
            }
        }

        if !found_target {
            bail!("No supported targets were found.");
        }

        Ok(CargoOutputs { outdir, metadata: artifacts, cargo_metadata: metadata })
    }

    pub fn cargo_metadata(&self, build_target: &str) -> Result<Metadata> {
        let mut cmd = MetadataCommand::new();

        // Use Kani's toolchain when running `cargo metadata`
        let cargo_path = get_cargo_path().unwrap();
        cmd.cargo_path(cargo_path);

        // restrict metadata command to host platform. References:
        // https://github.com/rust-lang/rust-analyzer/issues/6908
        // https://github.com/rust-lang/rust-analyzer/pull/6912
        cmd.other_options(vec![String::from("--filter-platform"), build_target.to_owned()]);

        // Set a --manifest-path if we're given one
        if let Some(path) = &self.args.cargo.manifest_path {
            cmd.manifest_path(path);
        }
        // Pass down features enables, which may affect dependencies or build metadata
        // (multiple calls to features are ok with cargo_metadata:)
        if self.args.cargo.all_features {
            cmd.features(cargo_metadata::CargoOpt::AllFeatures);
        }
        if self.args.cargo.no_default_features {
            cmd.features(cargo_metadata::CargoOpt::NoDefaultFeatures);
        }
        let features = self.args.cargo.features();
        if !features.is_empty() {
            cmd.features(cargo_metadata::CargoOpt::SomeFeatures(features));
        }

        cmd.exec().context("Failed to get cargo metadata.")
    }

    /// Run cargo and collect any error found.
    /// We also collect the metadata file generated during compilation if any.
    fn run_build(&self, cargo_cmd: Command) -> Result<Vec<RustcArtifact>> {
        let support_color = std::io::stdout().is_terminal();
        let mut artifacts = vec![];
        let mut cargo_process = self.run_piped(cargo_cmd)?;
        let reader = BufReader::new(cargo_process.stdout.take().unwrap());
        let mut error_count = 0;
        for message in Message::parse_stream(reader) {
            let message = message.unwrap();
            match message {
                Message::CompilerMessage(msg) => match msg.message.level {
                    DiagnosticLevel::FailureNote => {
                        print_msg(&msg.message, support_color)?;
                    }
                    DiagnosticLevel::Error => {
                        error_count += 1;
                        print_msg(&msg.message, support_color)?;
                    }
                    DiagnosticLevel::Ice => {
                        print_msg(&msg.message, support_color)?;
                        let _ = cargo_process.wait();
                        return Err(anyhow::Error::msg(msg.message).context(format!(
                            "Failed to compile `{}` due to an internal compiler error.",
                            msg.target.name
                        )));
                    }
                    _ => {
                        if !self.args.common_args.quiet {
                            print_msg(&msg.message, support_color)?;
                        }
                    }
                },
                Message::CompilerArtifact(rustc_artifact) => {
                    // Compares two targets, and falls back to a weaker
                    // comparison where we avoid dashes in their names.
                    artifacts.push(rustc_artifact)
                }
                Message::BuildScriptExecuted(_) | Message::BuildFinished(_) => {
                    // do nothing
                }
                Message::TextLine(msg) => {
                    if !self.args.common_args.quiet {
                        println!("{msg}");
                    }
                }

                // Non-exhaustive enum.
                _ => {
                    if !self.args.common_args.quiet {
                        println!("{message:?}");
                    }
                }
            }
        }
        let status = cargo_process.wait()?;
        if !status.success() {
            bail!("Failed to execute cargo ({status}). Found {error_count} compilation errors.");
        }
        Ok(artifacts)
    }

    /// Run cargo and collect any error found.
    /// We also collect the metadata file generated during compilation if any for the given target.
    fn run_build_target(&self, cargo_cmd: Command, target: &Target) -> Result<Option<Artifact>> {
        /// This used to be `rustc_artifact == *target`, but it
        /// started to fail after the `cargo` change in
        /// <https://github.com/rust-lang/cargo/pull/12783>
        ///
        /// We should revisit this check after a while to see if
        /// it's not needed anymore or it can be restricted to
        /// certain cases.
        /// TODO: <https://github.com/model-checking/kani/issues/3111>
        fn same_target(t1: &Target, t2: &Target) -> bool {
            (t1 == t2)
                || (t1.name.replace('-', "_") == t2.name.replace('-', "_")
                    && t1.kind == t2.kind
                    && t1.src_path == t2.src_path
                    && t1.edition == t2.edition
                    && t1.doctest == t2.doctest
                    && t1.test == t2.test
                    && t1.doc == t2.doc)
        }

        let compile_start = std::time::Instant::now();
        let artifacts = self.run_build(cargo_cmd)?;
        if std::env::var("TIME_COMPILER").is_ok() {
            // conditionally print the compilation time for debugging & use by `compile-timer`
            // doesn't just use the existing `--debug` flag because the number of prints significantly affects performance
            println!("BUILT {} IN {:?}μs", target.name, compile_start.elapsed().as_micros());
        }
        debug!(?artifacts, "run_build_target");

        // We generate kani specific artifacts only for the build target. The build target is
        // always the last artifact generated in a build, and all the other artifacts are related
        // to dependencies or build scripts.
        Ok(artifacts.into_iter().rev().find_map(|artifact| {
            if same_target(&artifact.target, target) { map_kani_artifact(artifact) } else { None }
        }))
    }

    /// Check that all package names are present in the workspace, otherwise return which aren't.
    fn to_package_ids<'a>(
        &self,
        package_names: &'a [String],
    ) -> Result<HashMap<PackageId, &'a str>> {
        package_names
            .iter()
            .map(|pkg| {
                let mut cmd = setup_cargo_command()?;
                cmd.arg("pkgid");
                if let Some(path) = &self.args.cargo.manifest_path {
                    cmd.arg("--manifest-path");
                    cmd.arg(path);
                }
                cmd.arg(pkg);
                // For some reason clippy cannot see that we are invoking wait() in the next line.
                #[allow(clippy::zombie_processes)]
                let mut process = self.run_piped(cmd)?;
                let result = process.wait()?;
                if !result.success() {
                    bail!("Failed to retrieve information for `{pkg}`");
                }

                let mut reader = BufReader::new(process.stdout.take().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line)?;
                trace!(package_id=?line, "package_ids");
                Ok((PackageId { repr: line.trim().to_string() }, pkg.as_str()))
            })
            .collect()
    }

    /// Extract the packages that should be verified.
    ///
    /// The result is built following these rules (mimicking cargo, see
    /// https://github.com/rust-lang/cargo/blob/master/src/cargo/core/workspace.rs):
    /// - If `--package <pkg>` is given, return the list of packages selected.
    /// - If `--exclude <pkg>` is given, return the list of packages not excluded.
    /// - If `--workspace` is given, return the list of workspace members.
    /// - Else obtain the set of packages from cargo's default_workspace_members (i.e., if
    ///   `default-members` is specified in Cargo.toml, use that list; else if a root package is
    ///   specified use that; else use all members).
    ///
    /// In addition, if either `--package <pkg>` or `--exclude <pkg>` is given,
    /// validate that `<pkg>` is a package name in the workspace, or return an error
    /// otherwise.
    fn packages_to_verify<'b>(
        &self,
        args: &VerificationArgs,
        metadata: &'b Metadata,
    ) -> Result<Vec<&'b Package>> {
        debug!(package_selection=?args.cargo.package, package_exclusion=?args.cargo.exclude, workspace=args.cargo.workspace, "packages_to_verify args");
        let packages = if !args.cargo.package.is_empty() {
            let pkg_ids = self.to_package_ids(&args.cargo.package)?;
            let filtered: Vec<_> = metadata
                .workspace_packages()
                .into_iter()
                .filter(|pkg| pkg_ids.contains_key(&pkg.id))
                .collect();
            if filtered.len() < args.cargo.package.len() {
                // Some packages specified in `--package` were not found in the workspace.
                let outer: Vec<_> = metadata
                    .packages
                    .iter()
                    .filter_map(|pkg| pkg_ids.get(&pkg.id).copied())
                    .collect();
                bail!(
                    "The following specified packages were not found in this workspace: `{}`",
                    outer.join("`,")
                );
            }
            filtered
        } else if !args.cargo.exclude.is_empty() {
            // should be ensured by argument validation
            assert!(args.cargo.workspace);
            let pkg_ids = self.to_package_ids(&args.cargo.exclude)?;
            metadata
                .workspace_packages()
                .into_iter()
                .filter(|pkg| !pkg_ids.contains_key(&pkg.id))
                .collect()
        } else if args.cargo.workspace {
            metadata.workspace_packages()
        } else {
            metadata.workspace_default_packages()
        };
        trace!(?packages, "packages_to_verify result");
        Ok(packages)
    }
}

pub fn cargo_config_args() -> Vec<CargoArg> {
    [
        "--target",
        env!("TARGET"),
        // Propagate `--cfg=kani_host` to build scripts.
        "-Zhost-config",
        "-Ztarget-applies-to-host",
        "--config=host.rustflags=[\"--cfg=kani_host\"]",
    ]
    .map(CargoArg::from)
    .to_vec()
}

/// Print the compiler message following the coloring schema.
fn print_msg(diagnostic: &Diagnostic, use_rendered: bool) -> Result<()> {
    if use_rendered {
        print!("{diagnostic}");
    } else {
        print!("{}", console::strip_ansi_codes(diagnostic.rendered.as_ref().unwrap()));
    }
    Ok(())
}

/// Extract Kani artifact that might've been generated from a given rustc artifact.
/// Not every rustc artifact will map to a kani artifact, hence the `Option<>`.
///
/// Unfortunately, we cannot always rely on the messages to get the path for the original artifact
/// that `rustc` produces. So we hack the content of the output path to point to the original
/// metadata file. See <https://github.com/model-checking/kani/issues/2234> for more details.
fn map_kani_artifact(rustc_artifact: cargo_metadata::Artifact) -> Option<Artifact> {
    debug!(?rustc_artifact, "map_kani_artifact");
    if rustc_artifact.target.is_custom_build() {
        // We don't verify custom builds.
        return None;
    }
    let result = rustc_artifact.filenames.iter().find_map(|path| {
        if path.extension() == Some("rmeta") {
            let file_stem = path.file_stem()?.strip_prefix("lib")?;
            let parent = path.parent().map(|p| p.as_std_path().to_path_buf()).unwrap_or_default();
            let mut meta_path = parent.join(file_stem);
            meta_path.set_extension(ArtifactType::Metadata);
            trace!(rmeta=?path, kani_meta=?meta_path.display(), "map_kani_artifact");

            // This will check if the file exists and we just skip if it doesn't.
            Artifact::try_new(&meta_path, ArtifactType::Metadata).ok()
        } else if path.extension() == Some("rlib") {
            // We skip `rlib` files since we should also generate a `rmeta`.
            trace!(rlib=?path, "map_kani_artifact");
            None
        } else {
            // For all the other cases we write the path of the metadata into the output file.
            // The compiler should always write a valid stub into the artifact file, however the
            // kani-metadata file only exists if there were valid targets.
            trace!(artifact=?path, "map_kani_artifact");
            let input_file = File::open(path).ok()?;
            let stub: CompilerArtifactStub = serde_json::from_reader(input_file).unwrap();
            Artifact::try_new(&stub.metadata_path, ArtifactType::Metadata).ok()
        }
    });
    debug!(?result, "map_kani_artifact");
    result
}

/// Possible verification targets.
#[derive(Debug)]
enum VerificationTarget {
    Bin(Target),
    Lib(Target),
    Test(Target),
}

impl VerificationTarget {
    /// Convert to cargo argument that select the specific target.
    fn to_args(&self) -> Vec<String> {
        match self {
            VerificationTarget::Test(target) => vec![String::from("--test"), target.name.clone()],
            VerificationTarget::Bin(target) => vec![String::from("--bin"), target.name.clone()],
            VerificationTarget::Lib(_) => vec![String::from("--lib")],
        }
    }

    fn target(&self) -> &Target {
        match self {
            VerificationTarget::Test(target)
            | VerificationTarget::Bin(target)
            | VerificationTarget::Lib(target) => target,
        }
    }
}

impl Display for VerificationTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerificationTarget::Test(target) => write!(f, "test `{}`", target.name),
            VerificationTarget::Bin(target) => write!(f, "binary `{}`", target.name),
            VerificationTarget::Lib(target) => write!(f, "lib `{}`", target.name),
        }
    }
}

/// Extract the targets inside a package.
///
/// If `--tests` is given, the list of targets will include any integration tests.
///
/// We use the `target.kind` as documented here. Note that `kind` for library will
/// match the `crate-type`, despite them not being explicitly listed in the documentation:
/// <https://docs.rs/cargo_metadata/0.15.0/cargo_metadata/struct.Target.html#structfield.kind>
///
/// The documentation for `crate-type` explicitly states that the only time `kind` and
/// `crate-type` differs is for examples.
/// <https://docs.rs/cargo_metadata/0.15.0/cargo_metadata/struct.Target.html#structfield.crate_types>
fn package_targets(args: &VerificationArgs, package: &Package) -> Vec<VerificationTarget> {
    let mut ignored_tests = vec![];
    let mut ignored_unsupported = vec![];
    let mut verification_targets = vec![];
    for target in &package.targets {
        debug!(name=?package.name, target=?target.name, kind=?target.kind, crate_type=?target
                .crate_types,
                "package_targets");
        let (mut supported_lib, mut unsupported_lib) = (false, false);
        for kind in &target.kind {
            match kind {
                TargetKind::Bin => {
                    if args.target.include_bin(&target.name) {
                        // Binary targets.
                        verification_targets.push(VerificationTarget::Bin(target.clone()));
                    }
                }
                TargetKind::Lib
                | TargetKind::RLib
                | TargetKind::CDyLib
                | TargetKind::DyLib
                | TargetKind::StaticLib => {
                    if args.target.include_lib() {
                        supported_lib = true;
                    }
                }
                TargetKind::ProcMacro => {
                    if args.target.include_lib() {
                        unsupported_lib = true;
                        ignored_unsupported.push(target.name.as_str());
                    }
                }
                TargetKind::Test => {
                    // Test target.
                    if args.target.include_tests() {
                        if args.tests {
                            verification_targets.push(VerificationTarget::Test(target.clone()));
                        } else {
                            ignored_tests.push(target.name.as_str());
                        }
                    }
                }
                _ => {
                    ignored_unsupported.push(target.name.as_str());
                }
            }
        }
        match (supported_lib, unsupported_lib) {
            (true, true) => println!(
                "warning: Skipped verification of `{}` due to unsupported crate-type: \
                        `proc-macro`.",
                target.name,
            ),
            (true, false) => verification_targets.push(VerificationTarget::Lib(target.clone())),
            (_, _) => {}
        }
    }

    if args.common_args.verbose {
        // Print targets that were skipped only on verbose mode.
        if !ignored_tests.is_empty() {
            println!("Skipped the following test targets: '{}'.", ignored_tests.join("', '"));
            println!("    -> Use '--tests' to verify harnesses inside a 'test' crate.");
        }
        if !ignored_unsupported.is_empty() {
            println!(
                "Skipped verification of the following unsupported targets: '{}'.",
                ignored_unsupported.join("', '")
            );
        }
    }
    verification_targets
}
