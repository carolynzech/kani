// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
//! This module contains code for resolving strings representing paths (simple and qualified) to
//! `DefId`s for functions and methods. For the definition of a path, see
//! <https://doc.rust-lang.org/reference/paths.html>.
//!
//! TODO: Change `resolve_fn` in order to return information about trait implementations.
//! <https://github.com/model-checking/kani/issues/1997>
//!
//! Note that glob use statements can form loops. The paths can also walk through the loop.

use crate::kani_middle::stable_fn_def;
use quote::ToTokens;
use rustc_errors::ErrorGuaranteed;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::def_id::{CRATE_DEF_INDEX, DefId, LOCAL_CRATE, LocalDefId, LocalModDefId};
use rustc_hir::{ItemKind, UseKind};
use rustc_middle::ty::TyCtxt;
use rustc_middle::ty::fast_reject::{self, TreatParams};
use rustc_public::CrateDef;
use rustc_public::rustc_internal;
use rustc_public::ty::{FnDef, RigidTy, Ty, TyKind};
use std::collections::HashSet;
use std::fmt;
use std::iter::Peekable;
use syn::{PathArguments, PathSegment, QSelf, TypePath};
use tracing::{debug, debug_span};

mod type_resolution;

macro_rules! validate_kind {
    ($tcx:ident, $id:ident, $expected:literal, $kind:pat) => {{
        let def_kind = $tcx.def_kind($id);
        if matches!(def_kind, $kind) {
            Ok($id)
        } else {
            Err(ResolveError::UnexpectedType { $tcx, item: $id, expected: $expected })
        }
    }};
}
pub(crate) use validate_kind;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FnResolution {
    Fn(FnDef),
    FnImpl { def: FnDef, ty: Ty },
}

/// Resolve a path to a function / method.
///
/// The path can either be a simple path or a qualified path.
pub fn resolve_fn_path<'tcx>(
    tcx: TyCtxt<'tcx>,
    current_module: LocalDefId,
    path: &TypePath,
) -> Result<FnResolution, ResolveError<'tcx>> {
    let _span = debug_span!("resolve_fn_path", ?path).entered();
    match &path.qself {
        // Qualified path for a trait method implementation, like `<Foo as Bar>::bar`.
        Some(QSelf { ty: syn_ty, position, .. }) if *position > 0 => {
            let ty = type_resolution::resolve_ty(tcx, current_module, syn_ty)?;
            let def_id = resolve_path(tcx, current_module, &path.path)?;
            validate_kind!(tcx, def_id, "function / method", DefKind::Fn | DefKind::AssocFn)?;
            Ok(FnResolution::FnImpl { def: stable_fn_def(tcx, def_id).unwrap(), ty })
        }
        // Qualified path for a primitive type, such as `<[u8]::sort>`.
        Some(QSelf { ty: syn_ty, .. }) if type_resolution::is_type_primitive(syn_ty) => {
            let ty = type_resolution::resolve_ty(tcx, current_module, syn_ty)?;
            let resolved = resolve_in_primitive(tcx, ty, path.path.segments.iter())?;
            if resolved.segments.is_empty() {
                Ok(FnResolution::Fn(stable_fn_def(tcx, resolved.base).unwrap()))
            } else {
                Err(ResolveError::UnexpectedType { tcx, item: resolved.base, expected: "module" })
            }
        }
        // Qualified path for a non-primitive type, such as `<Bar>::foo>`.
        Some(QSelf { ty: syn_ty, .. }) => {
            let ty = type_resolution::resolve_ty(tcx, current_module, syn_ty)?;
            let def_id = resolve_in_user_type(tcx, ty, path.path.segments.iter())?;
            validate_kind!(tcx, def_id, "function / method", DefKind::Fn | DefKind::AssocFn)?;
            Ok(FnResolution::Fn(stable_fn_def(tcx, def_id).unwrap()))
        }
        // Simple path
        None => {
            let def_id = resolve_path(tcx, current_module, &path.path)?;
            validate_kind!(tcx, def_id, "function / method", DefKind::Fn | DefKind::AssocFn)?;
            Ok(FnResolution::Fn(stable_fn_def(tcx, def_id).unwrap()))
        }
    }
}

/// Attempts to resolve a *simple path* (in the form of a string) to a function / method `DefId`.
///
/// Use `[resolve_fn_path]` if you want to handle qualified paths and simple paths.
pub fn resolve_fn<'tcx>(
    tcx: TyCtxt<'tcx>,
    current_module: LocalDefId,
    path_str: &str,
) -> Result<DefId, ResolveError<'tcx>> {
    let _span = debug_span!("resolve_fn", ?path_str, ?current_module).entered();
    let path = syn::parse_str(path_str).map_err(|err| ResolveError::InvalidPath {
        msg: format!("Expected a path, but found `{path_str}`. {err}"),
    })?;
    let result = resolve_fn_path(tcx, current_module, &path)?;
    if let FnResolution::Fn(def) = result {
        Ok(rustc_internal::internal(tcx, def.def_id()))
    } else {
        Err(ResolveError::UnsupportedPath { kind: "qualified paths" })
    }
}

/// Resolve the name of a function from the context of the definition provided.
///
/// Ideally this should pass a more precise span, but we don't keep them around.
pub fn expect_resolve_fn<T: CrateDef>(
    tcx: TyCtxt,
    res_cx: T,
    name: &str,
    reason: &str,
) -> Result<FnDef, ErrorGuaranteed> {
    let internal_def_id = rustc_internal::internal(tcx, res_cx.def_id());
    let current_module = tcx.parent_module_from_def_id(internal_def_id.as_local().unwrap());
    let maybe_resolved = resolve_fn(tcx, current_module.to_local_def_id(), name);
    let resolved = maybe_resolved.map_err(|err| {
        tcx.dcx().span_err(
            rustc_internal::internal(tcx, res_cx.span()),
            format!("Failed to resolve `{name}` for `{reason}`: {err}"),
        )
    })?;
    let ty_internal = tcx.type_of(resolved).instantiate_identity();
    let ty = rustc_internal::stable(ty_internal);
    if let TyKind::RigidTy(RigidTy::FnDef(def, _)) = ty.kind() {
        Ok(def)
    } else {
        unreachable!("Expected function for `{name}`, but found: {ty}")
    }
}

/// Attempts to resolve a simple path (in the form of a string) to a `DefId`.
/// The current module is provided as an argument in order to resolve relative
/// paths.
fn resolve_path<'tcx>(
    tcx: TyCtxt<'tcx>,
    current_module: LocalDefId,
    path: &syn::Path,
) -> Result<DefId, ResolveError<'tcx>> {
    debug!(?path, "resolve_path");
    let path = resolve_prefix(tcx, current_module, path)?;
    path.segments.into_iter().try_fold(path.base, |base, segment| {
        let name = segment.ident.to_string();
        let def_kind = tcx.def_kind(base);
        match def_kind {
            DefKind::ForeignMod | DefKind::Mod => resolve_in_module(tcx, base, &name),
            DefKind::Struct | DefKind::Enum | DefKind::Union => {
                resolve_in_type_def(tcx, base, &path.base_path_args, &name)
            }
            DefKind::Trait => resolve_in_trait(tcx, base, &name),
            kind => {
                debug!(?base, ?kind, "resolve_path: unexpected item");
                Err(ResolveError::UnexpectedType { tcx, item: base, expected: "module" })
            }
        }
    })
}

/// Provide information about where the resolution failed.
/// Todo: Add error message.
pub enum ResolveError<'tcx> {
    /// Ambiguous glob resolution.
    AmbiguousGlob { tcx: TyCtxt<'tcx>, name: String, base: DefId, candidates: Vec<DefId> },
    /// Ambiguous partial path (multiple inherent impls, c.f. https://github.com/model-checking/kani/issues/3773)
    AmbiguousPartialPath { tcx: TyCtxt<'tcx>, name: String, base: DefId, candidates: Vec<DefId> },
    /// Use super past the root of a crate.
    ExtraSuper,
    /// Invalid path.
    InvalidPath { msg: String },
    /// Unable to find an item.
    MissingItem { tcx: TyCtxt<'tcx>, base: DefId, unresolved: String },
    /// Unable to find an item in a primitive type.
    MissingPrimitiveItem { base: Ty, unresolved: String },
    /// Error triggered when the identifier points to an item with unexpected type.
    UnexpectedType { tcx: TyCtxt<'tcx>, item: DefId, expected: &'static str },
    /// Error triggered when the identifier is not currently supported.
    UnsupportedPath { kind: &'static str },
}

impl fmt::Debug for ResolveError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for ResolveError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::ExtraSuper => {
                write!(f, "there are too many leading `super` keywords")
            }
            ResolveError::AmbiguousGlob { tcx, base, name, candidates } => {
                let location = description(*tcx, *base);
                write!(
                    f,
                    "`{name}` is ambiguous because of multiple glob imports in {location}. Found:\n{}",
                    candidates
                        .iter()
                        .map(|def_id| tcx.def_path_str(*def_id))
                        .intersperse("\n".to_string())
                        .collect::<String>()
                )
            }
            ResolveError::AmbiguousPartialPath { tcx, base, name, candidates } => {
                let location = description(*tcx, *base);
                write!(
                    f,
                    "there are multiple implementations of {name} in {location}. Found:\n{}",
                    candidates
                        .iter()
                        .map(|def_id| tcx.def_path_str(*def_id))
                        .intersperse("\n".to_string())
                        .collect::<String>()
                )
            }
            ResolveError::InvalidPath { msg } => write!(f, "{msg}"),
            ResolveError::UnexpectedType { tcx, item: def_id, expected } => write!(
                f,
                "expected {expected}, found {} `{}`",
                tcx.def_kind(def_id).descr(*def_id),
                tcx.def_path_str(*def_id)
            ),
            ResolveError::MissingItem { tcx, base, unresolved } => {
                let def_desc = description(*tcx, *base);
                write!(f, "unable to find `{unresolved}` inside {def_desc}")
            }
            ResolveError::MissingPrimitiveItem { base, unresolved } => {
                write!(f, "unable to find `{unresolved}` inside `{base}`")
            }
            ResolveError::UnsupportedPath { kind } => {
                write!(f, "Kani currently cannot resolve {kind}")
            }
        }
    }
}

/// The segments of a path.
type Segments = Vec<PathSegment>;

/// A path consisting of a starting point, any PathArguments for that starting point, and a bunch of segments. If `base`
/// matches `Base::LocalModule { id: _, may_be_external_path : true }`, then
/// `segments` cannot be empty.
#[derive(Debug, Hash)]
struct Path {
    pub base: DefId,
    pub base_path_args: PathArguments,
    pub segments: Segments,
}

/// Identifier for the top module of the crate.
const CRATE: &str = "crate";
/// Identifier for the current module.
const SELF: &str = "self";
/// Identifier for the parent of the current module.
const SUPER: &str = "super";

/// Takes a string representation of a path and turns it into a `Path` data
/// structure, resolving prefix qualifiers (like `crate`, `self`, etc.) along the way.
fn resolve_prefix<'tcx>(
    tcx: TyCtxt<'tcx>,
    current_module: LocalDefId,
    path: &syn::Path,
) -> Result<Path, ResolveError<'tcx>> {
    debug!(?path, ?current_module, "resolve_prefix");

    // Split the string into segments separated by `::`. Trim the whitespace
    // since path strings generated from macros sometimes add spaces around
    // `::`.
    let mut segments = path.segments.iter();

    // Resolve qualifiers `crate`, initial `::`, and `self`. The qualifier
    // `self` may be followed be `super` (handled below).
    match (path.leading_colon, segments.next()) {
        // Leading `::` indicates that the path points to an item inside an external crate.
        (Some(_), Some(segment)) => {
            // Skip root and get the external crate from the name that follows `::`.
            let next_name = segment.ident.to_string();
            let result = resolve_external(tcx, &next_name);
            if let Some(def_id) = result {
                Ok(Path {
                    base: def_id,
                    base_path_args: segment.arguments.clone(),
                    segments: segments.cloned().collect(),
                })
            } else {
                Err(ResolveError::MissingItem {
                    tcx,
                    base: current_module.to_def_id(),
                    unresolved: next_name,
                })
            }
        }
        // Path with `::` alone is invalid.
        (Some(_), None) => {
            Err(ResolveError::InvalidPath { msg: "expected identifier after `::`".to_string() })
        }
        // Path starting with `crate::`.
        (None, Some(segment)) if segment.ident == CRATE => {
            // Find the module at the root of the crate.
            let current_module_hir_id = tcx.local_def_id_to_hir_id(current_module);
            let crate_root = match tcx.hir_parent_iter(current_module_hir_id).last() {
                None => current_module,
                Some((hir_id, _)) => hir_id.owner.def_id,
            };
            Ok(Path {
                base: crate_root.to_def_id(),
                base_path_args: segment.arguments.clone(),
                segments: segments.cloned().collect(),
            })
        }
        // Path starting with "self::"
        (None, Some(segment)) if segment.ident == SELF => {
            resolve_super(tcx, current_module, segments.peekable())
        }
        // Path starting with "super::"
        (None, Some(segment)) if segment.ident == SUPER => {
            resolve_super(tcx, current_module, path.segments.iter().peekable())
        }
        // Path starting with a primitive, such as "u8::"
        (None, Some(segment)) if type_resolution::is_primitive(&segment) => {
            let syn_ty = syn::parse2(segment.to_token_stream()).unwrap();
            let ty = type_resolution::resolve_ty(tcx, current_module, &syn_ty)?;
            resolve_in_primitive(tcx, ty, segments)
        }
        (None, Some(segment)) => {
            // No special key word was used. Try local first otherwise try external name.
            let next_name = segment.ident.to_string();
            let def_id =
                resolve_in_module(tcx, current_module.to_def_id(), &next_name).or_else(|err| {
                    if matches!(err, ResolveError::MissingItem { .. }) {
                        // Only try external if we couldn't find anything.
                        resolve_external(tcx, &next_name).ok_or(err)
                    } else {
                        Err(err)
                    }
                })?;
            Ok(Path {
                base: def_id,
                base_path_args: segment.arguments.clone(),
                segments: segments.cloned().collect(),
            })
        }
        _ => {
            unreachable!("Empty path: `{path:?}`")
        }
    }
}

/// Pop up the module stack until we account for all the `super` prefixes.
/// This method will error out if it tries to backtrace from the root crate.
fn resolve_super<'tcx, 'a, I>(
    tcx: TyCtxt,
    current_module: LocalDefId,
    mut segments: Peekable<I>,
) -> Result<Path, ResolveError<'tcx>>
where
    I: Iterator<Item = &'a PathSegment>,
{
    let current_module_hir_id = tcx.local_def_id_to_hir_id(current_module);
    let mut parents = tcx.hir_parent_iter(current_module_hir_id);
    let mut base_module = current_module;
    while segments.next_if(|segment| segment.ident == SUPER).is_some() {
        if let Some((parent, _)) = parents.next() {
            debug!("parent: {parent:?}");
            base_module = parent.owner.def_id;
        } else {
            return Err(ResolveError::ExtraSuper);
        }
    }
    debug!("base: {base_module:?}");
    Ok(Path {
        base: base_module.to_def_id(),
        base_path_args: PathArguments::None,
        segments: segments.cloned().collect(),
    })
}

/// Resolves an external crate name.
fn resolve_external(tcx: TyCtxt, name: &str) -> Option<DefId> {
    debug!(?name, "resolve_external");
    tcx.used_crates(()).iter().find_map(|crate_num| {
        let crate_name = tcx.crate_name(*crate_num);
        if crate_name.as_str() == name {
            Some(DefId { index: CRATE_DEF_INDEX, krate: *crate_num })
        } else {
            None
        }
    })
}

/// Resolves a path relative to a foreign module.
fn resolve_in_foreign_module(tcx: TyCtxt, foreign_mod: DefId, name: &str) -> Option<DefId> {
    debug!(?name, ?foreign_mod, "resolve_in_foreign_module");
    tcx.module_children(foreign_mod)
        .iter()
        .find_map(|item| if item.ident.as_str() == name { item.res.opt_def_id() } else { None })
}

/// Generates a more friendly string representation of a def_id including kind and name.
/// (the default representation for the crate root is the empty string).
fn description(tcx: TyCtxt, def_id: DefId) -> String {
    let def_kind = tcx.def_kind(def_id);
    let kind_name = def_kind.descr(def_id);
    if def_id.is_crate_root() {
        format!("{kind_name} `{}`", tcx.crate_name(LOCAL_CRATE))
    } else {
        format!("{kind_name} `{}`", tcx.def_path_str(def_id))
    }
}

/// The possible result of trying to resolve the name relative to a local module.
enum RelativeResolution {
    /// Return the item that user requested.
    Found(DefId),
    /// Return all globs that may define the item requested.
    Globs(Vec<Res>),
}

/// Resolves a path relative to a local module.
fn resolve_relative(tcx: TyCtxt, current_module: LocalModDefId, name: &str) -> RelativeResolution {
    debug!(?name, ?current_module, "resolve_relative");

    let mut glob_imports = vec![];
    let result = tcx.hir_module_free_items(current_module).find_map(|item_id| {
        let item = tcx.hir_item(item_id);
        if item.kind.ident().is_some_and(|ident| ident.as_str() == name) {
            match item.kind {
                ItemKind::Use(use_path, UseKind::Single(_)) => {
                    use_path.res.present_items().filter_map(|res| res.opt_def_id()).next()
                }
                ItemKind::ExternCrate(orig_name, _) => resolve_external(
                    tcx,
                    orig_name.as_ref().map(|sym| sym.as_str()).unwrap_or(name),
                ),
                _ => Some(item.owner_id.def_id.to_def_id()),
            }
        } else {
            if let ItemKind::Use(use_path, UseKind::Glob) = item.kind {
                // Do not immediately try to resolve the path using this glob,
                // since paths resolved via non-globs take precedence.
                glob_imports.extend(use_path.res.present_items());
            }
            None
        }
    });
    result.map_or(RelativeResolution::Globs(glob_imports), RelativeResolution::Found)
}

/// Resolves a path relative to a local or foreign module.
/// For local modules, if no module item matches the name we also have to traverse the list of glob
/// imports. For foreign modules, that list should've been flatten already.
fn resolve_in_module<'tcx>(
    tcx: TyCtxt<'tcx>,
    current_module: DefId,
    name: &str,
) -> Result<DefId, ResolveError<'tcx>> {
    match current_module.as_local() {
        None => resolve_in_foreign_module(tcx, current_module, name).ok_or_else(|| {
            ResolveError::MissingItem { tcx, base: current_module, unresolved: name.to_string() }
        }),
        Some(local_id) => {
            let result = resolve_relative(tcx, LocalModDefId::new_unchecked(local_id), name);
            match result {
                RelativeResolution::Found(def_id) => Ok(def_id),
                RelativeResolution::Globs(globs) => {
                    resolve_in_glob_uses(tcx, local_id, globs, name)
                }
            }
        }
    }
}

/// Resolves a path by exploring glob use statements.
/// Note that there could be loops in glob use statements, so we need to track modules that have
/// been visited.
fn resolve_in_glob_uses<'tcx>(
    tcx: TyCtxt<'tcx>,
    current_module: LocalDefId,
    mut glob_resolutions: Vec<Res>,
    name: &str,
) -> Result<DefId, ResolveError<'tcx>> {
    let mut visited = HashSet::<Res>::default();
    let mut matches = vec![];
    while let Some(res) = glob_resolutions.pop() {
        if !visited.contains(&res) {
            visited.insert(res);
            let result = resolve_in_glob_use(tcx, &res, name);
            match result {
                RelativeResolution::Found(def_id) => matches.push(def_id),
                RelativeResolution::Globs(mut other_globs) => {
                    glob_resolutions.append(&mut other_globs)
                }
            }
        }
    }
    match matches.len() {
        0 => Err(ResolveError::MissingItem {
            tcx,
            base: current_module.to_def_id(),
            unresolved: name.to_string(),
        }),
        1 => Ok(matches.pop().unwrap()),
        _ => Err(ResolveError::AmbiguousGlob {
            tcx,
            base: current_module.to_def_id(),
            name: name.to_string(),
            candidates: matches,
        }),
    }
}

/// Resolves a path by exploring a glob use statement.
fn resolve_in_glob_use(tcx: TyCtxt, res: &Res, name: &str) -> RelativeResolution {
    if let Res::Def(DefKind::Mod, def_id) = res {
        if let Some(local_id) = def_id.as_local() {
            resolve_relative(tcx, LocalModDefId::new_unchecked(local_id), name)
        } else {
            resolve_in_foreign_module(tcx, *def_id, name)
                .map_or(RelativeResolution::Globs(vec![]), RelativeResolution::Found)
        }
    } else {
        // This shouldn't happen. Only module imports can use globs.
        RelativeResolution::Globs(vec![])
    }
}

/// Resolves a function in a user type (non-primitive).
fn resolve_in_user_type<'tcx, 'a, I>(
    tcx: TyCtxt<'tcx>,
    ty: Ty,
    mut segments: I,
) -> Result<DefId, ResolveError<'tcx>>
where
    I: Iterator<Item = &'a PathSegment>,
{
    let def_id = match ty.kind() {
        TyKind::RigidTy(rigid_ty) => match rigid_ty {
            RigidTy::Adt(def, _) => rustc_internal::internal(tcx, def.def_id()),
            RigidTy::Foreign(_) => {
                return Err(ResolveError::UnsupportedPath { kind: "foreign type" });
            }
            _ => {
                unreachable!("Unexpected type {ty}")
            }
        },
        TyKind::Alias(_, _) => return Err(ResolveError::UnsupportedPath { kind: "alias" }),
        TyKind::Param(_) | TyKind::Bound(_, _) => {
            // Name resolution can not resolve in a parameter or bound.
            unreachable!()
        }
    };
    let Some(name) = segments.next() else { unreachable!() };
    if segments.next().is_some() {
        Err(ResolveError::UnexpectedType { tcx, item: def_id, expected: "module" })
    } else {
        resolve_in_type_def(tcx, def_id, &PathArguments::None, &name.ident.to_string())
    }
}

fn generic_args_to_string<T: ToTokens>(args: &T) -> String {
    args.to_token_stream().to_string().chars().filter(|c| !c.is_whitespace()).collect::<String>()
}

/// Resolves a function in a type given its `def_id`.
fn resolve_in_type_def<'tcx>(
    tcx: TyCtxt<'tcx>,
    type_id: DefId,
    base_path_args: &PathArguments,
    name: &str,
) -> Result<DefId, ResolveError<'tcx>> {
    debug!(?name, ?type_id, "resolve_in_type");
    // Try the inherent `impl` blocks (i.e., non-trait `impl`s).
    let candidates: Vec<DefId> = tcx
        .inherent_impls(type_id)
        .iter()
        .flat_map(|impl_id| tcx.associated_item_def_ids(impl_id))
        .cloned()
        .filter(|item| is_item_name(tcx, *item, name))
        .collect();

    match candidates.len() {
        0 => Err(ResolveError::MissingItem { tcx, base: type_id, unresolved: name.to_string() }),
        1 => Ok(candidates[0]),
        _ => {
            let invalid_path_err = |generic_args, candidates: Vec<DefId>| -> ResolveError {
                ResolveError::InvalidPath {
                    msg: format!(
                        "the generic arguments {} are invalid. The available implementations are: \n{}",
                        &generic_args,
                        &candidates
                            .iter()
                            .map(|def_id| tcx.def_path_str(def_id))
                            .intersperse("\n".to_string())
                            .collect::<String>()
                    ),
                }
            };
            // If there are multiple implementations, we need generic arguments on the base type to refine our options.
            match base_path_args {
                // If there aren't such arguments, report the ambiguity.
                PathArguments::None => Err(ResolveError::AmbiguousPartialPath {
                    tcx,
                    name: name.into(),
                    base: type_id,
                    candidates,
                }),
                // Otherwise, use the provided generic arguments to refine our options.
                PathArguments::AngleBracketed(args) => {
                    let generic_args = generic_args_to_string(&args);
                    let refined_candidates: Vec<DefId> = candidates
                        .iter()
                        .cloned()
                        .filter(|item| {
                            is_item_name_with_generic_args(tcx, *item, &generic_args, name)
                        })
                        .collect();
                    match refined_candidates.len() {
                        0 => Err(invalid_path_err(&generic_args, candidates)),
                        1 => Ok(refined_candidates[0]),
                        // since is_item_name_with_generic_args looks at the entire item path after the base type, it shouldn't be possible to have more than one match
                        _ => unreachable!(
                            "Got multiple refined candidates {:?}",
                            refined_candidates
                                .iter()
                                .map(|def_id| tcx.def_path_str(def_id))
                                .collect::<Vec<String>>()
                        ),
                    }
                }
                PathArguments::Parenthesized(args) => {
                    Err(invalid_path_err(&generic_args_to_string(args), candidates))
                }
            }
        }
    }
}

/// Resolves a function in a trait.
fn resolve_in_trait<'tcx>(
    tcx: TyCtxt<'tcx>,
    trait_id: DefId,
    name: &str,
) -> Result<DefId, ResolveError<'tcx>> {
    debug!(?name, ?trait_id, "resolve_in_trait");
    let missing_item_err =
        || ResolveError::MissingItem { tcx, base: trait_id, unresolved: name.to_string() };
    let trait_def = tcx.trait_def(trait_id);
    // Look for the given name in the list of associated items for the trait definition.
    tcx.associated_item_def_ids(trait_def.def_id)
        .iter()
        .copied()
        .find(|item| is_item_name(tcx, *item, name))
        .ok_or_else(missing_item_err)
}

/// Resolves a primitive type function.
///
/// This function assumes that `ty` is a primitive.
fn resolve_in_primitive<'tcx, 'a, I>(
    tcx: TyCtxt<'tcx>,
    ty: Ty,
    mut segments: I,
) -> Result<Path, ResolveError<'tcx>>
where
    I: Iterator<Item = &'a PathSegment>,
{
    if let Some(next) = segments.next() {
        let name = next.ident.to_string();
        debug!(?name, ?ty, "resolve_in_primitive");
        let internal_ty = rustc_internal::internal(tcx, ty);
        let simple_ty =
            fast_reject::simplify_type(tcx, internal_ty, TreatParams::InstantiateWithInfer)
                .unwrap();
        let impls = tcx.incoherent_impls(simple_ty);
        // Find the primitive impl.
        let item = impls
            .iter()
            .find_map(|item_impl| {
                tcx.associated_item_def_ids(item_impl)
                    .iter()
                    .copied()
                    .find(|item| is_item_name(tcx, *item, &name))
            })
            .ok_or_else(|| ResolveError::MissingPrimitiveItem {
                base: ty,
                unresolved: name.to_string(),
            })?;
        Ok(Path {
            base: item,
            base_path_args: PathArguments::None,
            segments: segments.cloned().collect(),
        })
    } else {
        Err(ResolveError::InvalidPath { msg: format!("Unexpected primitive type `{ty}`") })
    }
}

fn is_item_name(tcx: TyCtxt, item: DefId, name: &str) -> bool {
    let item_path = tcx.def_path_str(item);
    let last = item_path.split("::").last().unwrap();
    last == name
}

/// Use this when we don't just care about the item name matching (c.f. is_item_name),
/// but also if the generic arguments are the same, e.g. <u32>::unchecked_add.
fn is_item_name_with_generic_args(
    tcx: TyCtxt,
    item: DefId,
    generic_args: &str,
    name: &str,
) -> bool {
    let item_path = tcx.def_path_str(item);
    last_two_items_of_path_match(&item_path, generic_args, name)
}

// This is just a helper function for is_item_name_with_generic_args.
// It's in a separate function so we can unit-test it without a mock TyCtxt or DefIds.
fn last_two_items_of_path_match(item_path: &str, generic_args: &str, name: &str) -> bool {
    let parts: Vec<&str> = item_path.split("::").collect();

    if parts.len() < 2 {
        return false;
    }

    let actual_last_two =
        format!("{}{}{}{}", "::", parts[parts.len() - 2], "::", parts[parts.len() - 1]);

    let last_two = format!("{}{}{}", generic_args, "::", name);

    // The last two components of the item_path should be the same as ::{generic_args}::{name}
    last_two == actual_last_two
}

#[cfg(test)]
mod tests {
    mod simple_last_two_items_of_path_match {
        use crate::kani_middle::resolve::last_two_items_of_path_match;

        #[test]
        fn length_one_item_prefix() {
            let generic_args = "::<u32>";
            let name = "unchecked_add";
            let item_path = format!("NonZero{generic_args}::{name}");
            assert!(last_two_items_of_path_match(&item_path, generic_args, name))
        }

        #[test]
        fn length_three_item_prefix() {
            let generic_args = "::<u32>";
            let name = "unchecked_add";
            let item_path = format!("core::num::NonZero{generic_args}::{name}");
            assert!(last_two_items_of_path_match(&item_path, generic_args, name))
        }

        #[test]
        fn wrong_generic_arg() {
            let generic_args = "::<u64>";
            let name = "unchecked_add";
            let item_path = format!("core::num::NonZero{}::{}", "::<u32>", name);
            assert!(!last_two_items_of_path_match(&item_path, generic_args, name))
        }
    }
}
