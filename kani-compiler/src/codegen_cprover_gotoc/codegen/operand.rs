// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
use crate::codegen_cprover_gotoc::GotocCtx;
use crate::codegen_cprover_gotoc::utils::slice_fat_ptr;
use crate::kani_middle::is_anon_static;
use crate::unwrap_or_return_codegen_unimplemented;
use cbmc::goto_program::{DatatypeComponent, Expr, ExprValue, Location, Symbol, Type};
use rustc_middle::ty::Const as ConstInternal;
use rustc_public::mir::alloc::{AllocId, GlobalAlloc};
use rustc_public::mir::mono::{Instance, StaticDef};
use rustc_public::mir::{Mutability, Operand};
use rustc_public::rustc_internal;
use rustc_public::ty::{
    Allocation, ConstantKind, FloatTy, FnDef, GenericArgs, IntTy, MirConst, RigidTy, Size, Ty,
    TyConst, TyConstKind, TyKind, UintTy,
};
use rustc_public::{CrateDef, CrateItem};
use rustc_span::Span as SpanInternal;
use tracing::{debug, trace};

#[derive(Clone, Debug)]
enum AllocData<'a> {
    /// The data is represented as a slice of optional bytes, where None represents uninitialized
    /// bytes.
    Bytes(&'a [Option<u8>]),
    /// The allocation has been translated to an expression.
    Expr(Expr),
}

impl<'tcx> GotocCtx<'tcx> {
    /// Generate a goto expression from a MIR operand.
    ///
    /// A MIR operand is either a constant (literal or `const` declaration) or a place
    /// (being moved or copied for this operation).
    /// An "operand" in MIR is the argument to an "Rvalue" (and is also used by some statements.)
    pub fn codegen_operand_stable(&mut self, operand: &Operand) -> Expr {
        trace!(?operand, "codegen_operand");
        match operand {
            Operand::Copy(place) | Operand::Move(place) =>
            // TODO: move is an opportunity to poison/nondet the original memory.
            {
                let projection = unwrap_or_return_codegen_unimplemented!(
                    self,
                    self.codegen_place_stable(place, Location::none())
                );
                // If the operand itself is a Dynamic (like when passing a boxed closure),
                // we need to pull off the fat pointer. In that case, the rustc kind() on
                // both the operand and the inner type are Dynamic.
                // Consider moving this check elsewhere in:
                // https://github.com/model-checking/kani/issues/277
                match self.operand_ty_stable(operand).kind() {
                    TyKind::RigidTy(RigidTy::Dynamic(..)) => projection.fat_ptr_goto_expr.unwrap(),
                    _ => projection.goto_expr,
                }
            }
            Operand::Constant(constant) => {
                self.codegen_const(&constant.const_, self.codegen_span_stable(constant.span))
            }
        }
    }

    pub fn codegen_const_internal(
        &mut self,
        constant: ConstInternal<'tcx>,
        span: Option<SpanInternal>,
    ) -> Expr {
        let stable_const = rustc_internal::stable(constant);
        if let Some(stable_span) = rustc_internal::stable(span) {
            self.codegen_const_ty(&stable_const, self.codegen_span_stable(stable_span))
        } else {
            self.codegen_const_ty(&stable_const, Location::none())
        }
    }

    /// Generate a goto expression that represents a MIR-level constant.
    ///
    /// There are two possible constants included in the body of an instance:
    /// - Allocated: It will have its byte representation already defined. We try to eagerly
    ///   generate code for it as simple literals or constants if possible. Otherwise, we create
    ///   a memory allocation for them and access them indirectly.
    /// - ZeroSized: These are ZST constants and they just need to match the right type.
    pub fn codegen_const(&mut self, constant: &MirConst, loc: Location) -> Expr {
        trace!(?constant, "codegen_constant");
        match constant.kind() {
            ConstantKind::Allocated(alloc) => self.codegen_allocation(alloc, constant.ty(), loc),
            ConstantKind::ZeroSized => {
                let lit_ty = constant.ty();
                match lit_ty.kind() {
                    // Rust "function items" (not closures, not function pointers, see `codegen_fndef`)
                    TyKind::RigidTy(RigidTy::FnDef(def, args)) => {
                        self.codegen_fndef(def, &args, loc)
                    }
                    _ => Expr::init_unit(self.codegen_ty_stable(lit_ty), &self.symbol_table),
                }
            }
            ConstantKind::Param(..) | ConstantKind::Unevaluated(..) => {
                unreachable!()
            }
            ConstantKind::Ty(t) => self.codegen_const_ty(t, loc),
        }
    }

    /// Generate a goto expression that represents a type-level constant.
    ///
    /// There are two possible constants included in the body of an instance:
    /// - Allocated: It will have its byte representation already defined. We try to eagerly
    ///   generate code for it as simple literals or constants if possible. Otherwise, we create
    ///   a memory allocation for them and access them indirectly.
    /// - ZeroSized: These are ZST constants and they just need to match the right type.
    pub fn codegen_const_ty(&mut self, constant: &TyConst, loc: Location) -> Expr {
        trace!(?constant, "codegen_constant");
        match constant.kind() {
            TyConstKind::ZSTValue(lit_ty) => {
                match lit_ty.kind() {
                    // Rust "function items" (not closures, not function pointers, see `codegen_fndef`)
                    TyKind::RigidTy(RigidTy::FnDef(def, args)) => {
                        self.codegen_fndef(def, &args, loc)
                    }
                    _ => Expr::init_unit(self.codegen_ty_stable(*lit_ty), &self.symbol_table),
                }
            }
            TyConstKind::Value(ty, alloc) => self.codegen_allocation(alloc, *ty, loc),
            TyConstKind::Bound(..) => unreachable!(),
            TyConstKind::Param(..) | TyConstKind::Unevaluated(..) => {
                unreachable!()
            }
        }
    }

    pub fn codegen_allocation(&mut self, alloc: &Allocation, ty: Ty, loc: Location) -> Expr {
        // First try to generate the constant without allocating memory.
        let expr = self.try_codegen_constant(alloc, ty, loc).unwrap_or_else(|| {
            debug!("codegen_allocation try_fail");
            let mem_var = self.codegen_const_allocation(alloc, None, loc);
            mem_var
                .cast_to(Type::unsigned_int(8).to_pointer())
                .cast_to(self.codegen_ty_stable(ty).to_pointer())
                .dereference()
        });
        debug!(?expr, ?alloc, ?ty, "codegen_allocation");
        expr
    }

    /// Before allocating space for a constant, try to generate a simple expression.
    ///
    /// Generate an expression for a constant too small/simple to require an `Allocation` such as:
    /// 1. integers
    /// 2. ZST, or transparent structs of one (scalar) value
    /// 3. enums that don't carry data
    /// 4. unit, tuples (may be multi-ary!), or size-0 arrays
    /// 5. pointers to an allocation
    fn try_codegen_constant(&mut self, alloc: &Allocation, ty: Ty, loc: Location) -> Option<Expr> {
        debug!(?alloc, ?ty, "try_codegen_constant");
        match ty.kind() {
            TyKind::RigidTy(RigidTy::Int(it)) => {
                let val = alloc.read_int().unwrap();
                Some(match it {
                    IntTy::Isize => Expr::ssize_constant(val, &self.symbol_table),
                    IntTy::I8 => Expr::int_constant(val as i8, Type::signed_int(8)),
                    IntTy::I16 => Expr::int_constant(val as i16, Type::signed_int(16)),
                    IntTy::I32 => Expr::int_constant(val as i32, Type::signed_int(32)),
                    IntTy::I64 => Expr::int_constant(val as i64, Type::signed_int(64)),
                    IntTy::I128 => Expr::int_constant(val, Type::signed_int(128)),
                })
            }
            TyKind::RigidTy(RigidTy::Uint(it)) => {
                let val = alloc.read_uint().unwrap();
                Some(match it {
                    UintTy::Usize => Expr::size_constant(val, &self.symbol_table),
                    UintTy::U8 => Expr::int_constant(val as u8, Type::unsigned_int(8)),
                    UintTy::U16 => Expr::int_constant(val as u16, Type::unsigned_int(16)),
                    UintTy::U32 => Expr::int_constant(val as u32, Type::unsigned_int(32)),
                    UintTy::U64 => Expr::int_constant(val as u64, Type::unsigned_int(64)),
                    UintTy::U128 => Expr::int_constant(val, Type::unsigned_int(128)),
                })
            }
            TyKind::RigidTy(RigidTy::Bool) => {
                Some(Expr::c_bool_constant(alloc.read_bool().unwrap()))
            }
            TyKind::RigidTy(RigidTy::Char) => {
                Some(Expr::int_constant(alloc.read_int().unwrap(), Type::signed_int(32)))
            }
            TyKind::RigidTy(RigidTy::Float(k)) =>
            // rustc uses a sophisticated format for floating points that is hard to get f32/f64 from.
            // Instead, we use integers with the right width to represent the bit pattern.
            {
                match k {
                    FloatTy::F16 => Some(Expr::float16_constant_from_bitpattern(
                        alloc.read_uint().unwrap() as u16,
                    )),
                    FloatTy::F32 => Some(Expr::float_constant_from_bitpattern(
                        alloc.read_uint().unwrap() as u32,
                    )),
                    FloatTy::F64 => Some(Expr::double_constant_from_bitpattern(
                        alloc.read_uint().unwrap() as u64,
                    )),
                    FloatTy::F128 => {
                        Some(Expr::float128_constant_from_bitpattern(alloc.read_uint().unwrap()))
                    }
                }
            }
            TyKind::RigidTy(RigidTy::RawPtr(inner_ty, _))
            | TyKind::RigidTy(RigidTy::Ref(_, inner_ty, _)) => {
                Some(self.codegen_const_ptr(alloc, ty, inner_ty, loc))
            }
            TyKind::RigidTy(RigidTy::Adt(adt, args)) if adt.kind().is_struct() => {
                //Special struct that is used to handle type_id function
                if adt.name().contains("any::TypeId") {
                    let val = alloc.read_uint().unwrap();
                    let u128_expr = Expr::int_constant(val, Type::unsigned_int(128));
                    let typ = self.codegen_ty_stable(ty);
                    return Some(u128_expr.transmute_to(typ, &self.symbol_table));
                }
                // Structs only have one variant.
                let variant = adt.variants_iter().next().unwrap();
                // There must be at least one field associated with the scalar data.
                // Any additional fields correspond to ZSTs.
                let field_types: Vec<_> =
                    variant.fields().iter().map(|f| f.ty_with_args(&args)).collect();
                // Check that there is a single non-ZST field.
                let non_zst_types: Vec<_> =
                    field_types.iter().filter(|t| !self.is_zst_stable(**t)).collect();
                debug!(len=?non_zst_types.len(), "non_zst_types");
                if non_zst_types.len() == 1 {
                    // Only try to directly expand the constant if only one field has data.
                    // We could eventually expand this, but keep it simple for now. See:
                    // https://github.com/model-checking/kani/issues/2936
                    let overall_type = self.codegen_ty_stable(ty);
                    let field_values: Vec<Expr> = field_types
                        .iter()
                        .map(|t| {
                            if self.is_zst_stable(*t) {
                                Some(Expr::init_unit(
                                    self.codegen_ty_stable(*t),
                                    &self.symbol_table,
                                ))
                            } else {
                                self.try_codegen_constant(alloc, *t, loc)
                            }
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some(Expr::struct_expr_from_values(
                        overall_type,
                        field_values,
                        &self.symbol_table,
                    ))
                } else {
                    // Structures with more than one non-ZST element are handled with an extra
                    // allocation.
                    None
                }
            }
            TyKind::RigidTy(RigidTy::Tuple(tys)) if tys.len() == 1 => {
                let overall_t = self.codegen_ty_stable(ty);
                let inner_expr = self.try_codegen_constant(alloc, tys[0], loc)?;
                Some(inner_expr.transmute_to(overall_t, &self.symbol_table))
            }
            // Everything else we encode as an allocation.
            _ => None,
        }
    }

    fn codegen_const_ptr(
        &mut self,
        alloc: &Allocation,
        ty: Ty,
        inner_ty: Ty,
        loc: Location,
    ) -> Expr {
        debug!(?ty, ?alloc, "codegen_const_ptr");
        if self.use_fat_pointer_stable(inner_ty) {
            match inner_ty.kind() {
                TyKind::RigidTy(RigidTy::Str) => {
                    // a string literal
                    // Create a static variable that holds its value
                    assert_eq!(
                        alloc.provenance.ptrs.len(),
                        1,
                        "Expected `&str` to point to a str buffer"
                    );
                    let alloc_id = alloc.provenance.ptrs[0].1.0;
                    let GlobalAlloc::Memory(data) = GlobalAlloc::from(alloc_id) else {
                        unreachable!()
                    };
                    let mem_var = self.codegen_const_allocation(&data, None, loc);

                    // Extract identifier for static variable.
                    // codegen_allocation_auto_imm_name returns the *address* of
                    // the variable, so need to pattern match to extract it.
                    let ident = match mem_var.value() {
                        ExprValue::AddressOf(address) => match address.value() {
                            ExprValue::Symbol { identifier } => identifier,
                            _ => unreachable!("Expecting a symbol for a string literal allocation"),
                        },
                        _ => unreachable!("Expecting an address for string literal allocation"),
                    };

                    // Extract the actual string literal
                    let bytes = data.raw_bytes().unwrap();
                    let s = ::std::str::from_utf8(&bytes).expect("non utf8 str from mir");

                    // Store the identifier to the string literal in the goto context
                    self.str_literals.insert(*ident, s.into());

                    // Codegen as a fat pointer
                    let data_expr = mem_var.cast_to(Type::unsigned_int(8).to_pointer());
                    let len_expr = Expr::int_constant(bytes.len(), Type::size_t());
                    slice_fat_ptr(
                        self.codegen_ty_stable(ty),
                        data_expr,
                        len_expr,
                        &self.symbol_table,
                    )
                }
                TyKind::RigidTy(RigidTy::Slice(inner_ty)) => {
                    // Create a static variable that holds its value
                    assert_eq!(
                        alloc.provenance.ptrs.len(),
                        1,
                        "Expected `&[T]` to point to a single buffer"
                    );
                    let alloc_id = alloc.provenance.ptrs[0].1.0;
                    let GlobalAlloc::Memory(data) = GlobalAlloc::from(alloc_id) else {
                        unreachable!()
                    };
                    let mem_var = self.codegen_const_allocation(&data, None, loc);
                    let inner_typ = self.codegen_ty_stable(inner_ty);
                    let len = data.bytes.len() / inner_typ.sizeof(&self.symbol_table) as usize;
                    let data_expr = mem_var.cast_to(inner_typ.to_pointer());
                    let len_expr = Expr::int_constant(len, Type::size_t());
                    slice_fat_ptr(
                        self.codegen_ty_stable(ty),
                        data_expr,
                        len_expr,
                        &self.symbol_table,
                    )
                }
                // TODO: Improve this check after we upgrade nightly to 2023-12-18.
                TyKind::RigidTy(RigidTy::Adt(def, _)) if def.name().ends_with("::CStr") => {
                    // TODO: Handle CString
                    // <https://github.com/model-checking/kani/issues/2549>
                    let typ = self.codegen_ty_stable(ty);
                    let operation_name = "C string literal";
                    self.codegen_unimplemented_expr(
                        operation_name,
                        typ,
                        loc,
                        "https://github.com/model-checking/kani/issues/2549",
                    )
                }
                _ => unreachable!("{inner_ty:?}"),
            }
        } else if !alloc.provenance.ptrs.is_empty() {
            // Codegen the provenance pointer.
            trace!("codegen_const_ptr with_prov");
            let ptr = alloc.provenance.ptrs[0];
            let alloc_id = ptr.1.0;
            let typ = self.codegen_ty_stable(ty);
            self.codegen_alloc_pointer(typ, alloc_id, ptr.0, loc)
        } else {
            // If there's no provenance, just codegen the pointer address.
            trace!("codegen_const_ptr no_prov");
            let expr = Expr::size_constant(alloc.read_uint().unwrap(), &self.symbol_table);
            expr.cast_to(self.codegen_ty_stable(ty))
        }
    }

    /// A private helper function that ensures `alloc_id` is "allocated" (exists in the global symbol table and is
    /// initialized), and just returns a pointer to somewhere (using `offset`) inside it.
    fn codegen_alloc_pointer(
        &mut self,
        res_t: Type,
        alloc_id: AllocId,
        offset: Size,
        loc: Location,
    ) -> Expr {
        debug!(?res_t, ?alloc_id, "codegen_alloc_pointer");
        let base_addr = match GlobalAlloc::from(alloc_id) {
            GlobalAlloc::Function(instance) => {
                // We want to return the function pointer (not to be confused with function item)
                self.codegen_func_expr(instance, loc).address_of()
            }
            GlobalAlloc::Static(def) => {
                if is_anon_static(self.tcx, def.def_id()) {
                    let alloc = def.eval_initializer().unwrap();
                    let name = format!("{}::{alloc_id:?}", self.full_crate_name());
                    self.codegen_nested_static_allocation(&alloc, Some(name), loc)
                } else {
                    self.codegen_static_pointer(def)
                }
            }
            GlobalAlloc::Memory(alloc) => {
                // Full (mangled) crate name added so that allocations from different
                // crates do not conflict. The name alone is insufficient because Rust
                // allows different versions of the same crate to be used.
                let name = format!("{}::{alloc_id:?}", self.full_crate_name());
                self.codegen_const_allocation(&alloc, Some(name), loc)
            }
            alloc @ GlobalAlloc::VTable(..) => {
                // This is similar to GlobalAlloc::Memory but the type is opaque to rust and it
                // requires a bit more logic to get information about the allocation.
                let vtable_alloc_id = alloc.vtable_allocation().unwrap();
                let GlobalAlloc::Memory(alloc) = GlobalAlloc::from(vtable_alloc_id) else {
                    unreachable!()
                };
                let name = format!("{}::{alloc_id:?}", self.full_crate_name());
                self.codegen_const_allocation(&alloc, Some(name), loc)
            }
            GlobalAlloc::TypeId { ty: _ } => todo!(),
        };
        assert!(res_t.is_pointer() || res_t.is_transparent_type(&self.symbol_table));
        let offset_addr = base_addr
            .cast_to(Type::unsigned_int(8).to_pointer())
            .plus(Expr::int_constant(offset, Type::unsigned_int(64)));

        // In some cases, Rust uses a transparent type here. Convert the pointer to an rvalue
        // of the type expected. https://github.com/model-checking/kani/issues/822
        if let Some(wrapped_type) = res_t.unwrap_transparent_type(&self.symbol_table) {
            assert!(wrapped_type.is_pointer());
            offset_addr
                .cast_to(wrapped_type)
                .transmute_to_structurally_equivalent_type(res_t, &self.symbol_table)
        } else {
            assert!(res_t.is_pointer());
            offset_addr.cast_to(res_t)
        }
    }

    /// Generate a goto expression for a pointer to a static.
    ///
    /// These are not initialized here, see `codegen_static`.
    fn codegen_static_pointer(&mut self, def: StaticDef) -> Expr {
        self.codegen_instance_pointer(Instance::from(def), false)
    }

    /// Generate a goto expression for a pointer to a thread-local variable.
    ///
    /// These are not initialized here, see `codegen_static`.
    pub fn codegen_thread_local_pointer(&mut self, def: CrateItem) -> Expr {
        let instance = Instance::try_from(def).unwrap();
        self.codegen_instance_pointer(instance, true)
    }

    /// Generate a goto expression for a pointer to a static or thread-local variable.
    fn codegen_instance_pointer(&mut self, instance: Instance, is_thread_local: bool) -> Expr {
        let sym = self.ensure(instance.mangled_name(), |ctx, name| {
            // Rust has a notion of "extern static" variables. These are in an "extern" block,
            // and so aren't initialized in the current codegen unit. For example (from std):
            //      extern "C" {
            //          #[linkage = "extern_weak"]
            //          static __dso_handle: *mut u8;
            //          #[linkage = "extern_weak"]
            //          static __cxa_thread_atexit_impl: *const libc::c_void;
            //      }
            // CBMC shares C's notion of "extern" global variables. However, CBMC mostly does
            // not use this information except when doing C typechecking.
            // The one exception is handling static variables with no initializer (see
            // CBMC's `static_lifetime_init`):
            //   1. If they are `is_extern` they are nondet-initialized.
            //   2. If they are `!is_extern`, they are zero-initialized.
            // So we recognize a Rust "extern" declaration and pass that information along.
            let is_extern = instance.is_foreign_item();

            let span = instance.def.span();
            Symbol::static_variable(
                name.to_string(),
                name.to_string(),
                ctx.codegen_ty_stable(instance.ty()),
                ctx.codegen_span_stable(span),
            )
            .with_is_extern(is_extern)
            .with_is_thread_local(is_thread_local)
        });
        sym.clone().to_expr().address_of()
    }

    /// Generate an expression that represents the address for a constant allocation.
    ///
    /// This function will only allocate a new memory location if necessary. The standard does
    /// not offer any guarantees over the location of a constant.
    ///
    /// These constants can be named constants which are declared by the user, or constant values
    /// used scattered throughout the source
    fn codegen_const_allocation(
        &mut self,
        alloc: &Allocation,
        name: Option<String>,
        loc: Location,
    ) -> Expr {
        debug!(?name, ?alloc, "codegen_const_allocation");
        let alloc_name = match self.alloc_map.get(alloc) {
            None => {
                let alloc_name = if let Some(name) = name { name } else { self.next_global_name() };
                let has_interior_mutabity = false; // Constants cannot be mutated.
                self.codegen_alloc_in_memory(
                    alloc.clone(),
                    alloc_name.clone(),
                    loc,
                    has_interior_mutabity,
                );
                alloc_name
            }
            Some(name) => name.clone(),
        };

        let mem_place = self.symbol_table.lookup(alloc_name).unwrap().to_expr();
        mem_place.address_of()
    }

    /// Generate an expression that represents the address of a nested static allocation.
    fn codegen_nested_static_allocation(
        &mut self,
        alloc: &Allocation,
        name: Option<String>,
        loc: Location,
    ) -> Expr {
        // The memory behind this allocation isn't constant, but codegen_alloc_in_memory (which codegen_const_allocation calls)
        // uses alloc's mutability field to set the const-ness of the allocation in CBMC's symbol table,
        // so we can reuse the code and without worrying that the allocation is set as immutable.
        self.codegen_const_allocation(alloc, name, loc)
    }

    /// Insert an allocation into the goto symbol table, and generate an init value.
    ///
    /// This function is ultimately responsible for creating new statically initialized global
    /// variables.
    pub fn codegen_alloc_in_memory(
        &mut self,
        alloc: Allocation,
        name: String,
        loc: Location,
        has_interior_mutabity: bool,
    ) {
        debug!(?name, ?alloc, "codegen_alloc_in_memory");
        let struct_name = &format!("{name}::struct");

        // The declaration of a static variable may have one type and the constant initializer for
        // a static variable may have a different type. This is because Rust uses bit patterns for
        // initializers. For example, for a boolean static variable, the variable will have type
        // CBool and the initializer will be a single byte (a one-character array) representing the
        // bit pattern for the boolean value.
        let alloc_data = self.codegen_allocation_data(&alloc, loc);
        let alloc_typ_ref = self.ensure_struct(struct_name, struct_name, |_, _| {
            alloc_data
                .iter()
                .enumerate()
                .map(|(i, d)| match d {
                    AllocData::Bytes(bytes) => DatatypeComponent::field(
                        i.to_string(),
                        Type::unsigned_int(8).array_of(bytes.len()),
                    ),
                    AllocData::Expr(e) => DatatypeComponent::field(i.to_string(), e.typ().clone()),
                })
                .collect()
        });

        // Create the allocation from a byte array.
        let init_fn = |gcx: &mut GotocCtx, var: Symbol| {
            let val = Expr::struct_expr_from_values(
                alloc_typ_ref.clone(),
                alloc_data
                    .iter()
                    .map(|d| match d {
                        AllocData::Bytes(bytes) => Expr::array_expr(
                            Type::unsigned_int(8).array_of(bytes.len()),
                            bytes
                                .iter()
                                // We should consider adding a poison / undet where we have none
                                // This mimics the behaviour before StableMIR though.
                                .map(|b| Expr::int_constant(b.unwrap_or(0), Type::unsigned_int(8)))
                                .collect(),
                        ),
                        AllocData::Expr(e) => e.clone(),
                    })
                    .collect(),
                &gcx.symbol_table,
            );
            if val.typ() == &var.typ { val } else { val.transmute_to(var.typ, &gcx.symbol_table) }
        };

        // The global static variable may not be in the symbol table if we are dealing
        // with a promoted constant.
        let _var = self.ensure_global_var_init(
            &name,
            false, //TODO is this correct?
            alloc.mutability == Mutability::Not && !has_interior_mutabity,
            alloc_typ_ref.clone(),
            loc,
            init_fn,
        );

        self.alloc_map.insert(alloc, name);
    }

    /// This is an internal helper function for `codegen_alloc_in_memory`.
    ///
    /// We codegen global statics as their own unique struct types, and this creates a field-by-field
    /// representation of what those fields should be initialized with.
    /// (A field is either bytes, or initialized with an expression.)
    fn codegen_allocation_data<'a>(
        &mut self,
        alloc: &'a Allocation,
        loc: Location,
    ) -> Vec<AllocData<'a>> {
        let mut alloc_vals = Vec::with_capacity(alloc.provenance.ptrs.len() + 1);
        let pointer_size = self.symbol_table.machine_model().pointer_width_in_bytes();

        let mut next_offset = 0;
        for &(offset, prov) in alloc.provenance.ptrs.iter() {
            if offset > next_offset {
                let bytes = &alloc.bytes[next_offset..offset];
                alloc_vals.push(AllocData::Bytes(bytes));
            }
            let ptr_offset = { alloc.read_partial_uint(offset..(offset + pointer_size)).unwrap() };
            alloc_vals.push(AllocData::Expr(self.codegen_alloc_pointer(
                Type::signed_int(8).to_pointer(),
                prov.0,
                ptr_offset.try_into().unwrap(),
                loc,
            )));

            next_offset = offset + pointer_size;
        }
        if alloc.bytes.len() >= next_offset {
            let range = next_offset..alloc.bytes.len();
            let bytes = &alloc.bytes[range];
            alloc_vals.push(AllocData::Bytes(bytes));
        }

        alloc_vals
    }

    /// Returns `Some(instance)` if the function is an intrinsic; `None` otherwise
    pub fn get_instance(&self, func: &Operand) -> Option<Instance> {
        let funct = self.operand_ty_stable(func);
        match funct.kind() {
            TyKind::RigidTy(RigidTy::FnDef(def, args)) => {
                Some(Instance::resolve(def, &args).unwrap())
            }
            _ => None,
        }
    }

    /// Generate a goto expression for a MIR "function item" reference.
    ///
    /// A "function item" is a ZST that corresponds to a specific single function.
    /// This is not the closure, nor a function pointer.
    ///
    /// Unlike closures or pointers, which can point to anything of the correct type,
    /// a function item is a type associated with a unique function.
    /// This type has impls for e.g. Fn, FnOnce, etc, which is how it safely converts to other
    /// function types.
    ///
    /// See <https://doc.rust-lang.org/reference/types/function-item.html>
    pub fn codegen_fndef(&mut self, def: FnDef, args: &GenericArgs, loc: Location) -> Expr {
        let instance = Instance::resolve(def, args).unwrap();
        self.codegen_fn_item(instance, loc)
    }

    /// Ensure that the given instance is in the symbol table, returning the symbol.
    fn codegen_func_symbol(&mut self, instance: Instance) -> &Symbol {
        if instance.is_foreign_item() && !instance.has_body() {
            // Get the symbol that represents a foreign instance.
            self.codegen_foreign_fn(instance)
        } else {
            // All non-foreign functions should've been declared beforehand.
            trace!(func=?instance, "codegen_func_symbol");
            let func = instance.mangled_name();
            self.symbol_table
                .lookup(&func)
                .unwrap_or_else(|| panic!("Function `{func}` should've been declared before usage"))
        }
    }

    /// Generate a goto expression that references the function identified by `instance`.
    ///
    /// Note: In general with this `Expr` you should immediately either `.address_of()` or `.call(...)`.
    ///
    /// This should not be used where Rust expects a "function item" (See `codegen_fn_item`)
    pub fn codegen_func_expr(&mut self, instance: Instance, loc: Location) -> Expr {
        let func_symbol = self.codegen_func_symbol(instance);
        Expr::symbol_expression(func_symbol.name, func_symbol.typ.clone()).with_location(loc)
    }

    /// Generate a goto expression referencing the singleton value for a MIR "function item".
    ///
    /// For a given function instance, generate a ZST struct and return a singleton reference to that.
    /// This is the Rust "function item". See <https://doc.rust-lang.org/reference/types/function-item.html>
    /// This is not the function pointer, for that use `codegen_func_expr`.
    fn codegen_fn_item(&mut self, instance: Instance, loc: Location) -> Expr {
        let func_symbol = self.codegen_func_symbol(instance);
        let mangled_name = func_symbol.name;
        let fn_item_struct_ty = self.codegen_fndef_type_stable(instance);
        // This zero-sized object that a function name refers to in Rust is globally unique, so we create such a global object.
        let fn_singleton_name = format!("{mangled_name}::FnDefSingleton");
        self.ensure_global_var(&fn_singleton_name, false, fn_item_struct_ty, loc).to_expr()
    }
}
