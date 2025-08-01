// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Single source of truth about which intrinsics we support.

use rustc_public::{
    mir::{Mutability, mono::Instance},
    ty::{FloatTy, IntTy, RigidTy, TyKind, UintTy},
};

// Enumeration of all intrinsics we support right now, with the last option being a catch-all. This
// way, adding an intrinsic would highlight all places where they are used.
#[allow(unused)]
#[derive(Clone, Debug)]
pub enum Intrinsic {
    AddWithOverflow,
    AlignOfVal,
    ArithOffset,
    AssertInhabited,
    AssertMemUninitializedValid,
    AssertZeroValid,
    Assume,
    AtomicAnd,
    AtomicCxchg,
    AtomicCxchgWeak,
    AtomicFence,
    AtomicLoad,
    AtomicMax,
    AtomicMin,
    AtomicNand,
    AtomicOr,
    AtomicSingleThreadFence,
    AtomicStore,
    AtomicUmax,
    AtomicUmin,
    AtomicXadd,
    AtomicXchg,
    AtomicXor,
    AtomicXsub,
    Bitreverse,
    BlackBox,
    Breakpoint,
    Bswap,
    CeilF32,
    CeilF64,
    CompareBytes,
    Copy,
    CopySignF32,
    CopySignF64,
    CosF32,
    CosF64,
    Ctlz,
    CtlzNonZero,
    Ctpop,
    Cttz,
    CttzNonZero,
    DiscriminantValue,
    ExactDiv,
    Exp2F32,
    Exp2F64,
    ExpF32,
    ExpF64,
    FabsF32,
    FabsF64,
    FaddFast,
    FdivFast,
    FloatToIntUnchecked,
    FloorF32,
    FloorF64,
    FmafF32,
    FmafF64,
    FmulFast,
    Forget,
    FsubFast,
    IsValStaticallyKnown,
    Likely,
    Log10F32,
    Log10F64,
    Log2F32,
    Log2F64,
    LogF32,
    LogF64,
    MaxNumF32,
    MaxNumF64,
    MinNumF32,
    MinNumF64,
    MulWithOverflow,
    PowF32,
    PowF64,
    PowIF32,
    PowIF64,
    PtrGuaranteedCmp,
    PtrOffsetFrom,
    PtrOffsetFromUnsigned,
    RawEq,
    RetagBoxToRaw,
    RotateLeft,
    RotateRight,
    RoundF32,
    RoundF64,
    RoundTiesEvenF32,
    RoundTiesEvenF64,
    SaturatingAdd,
    SaturatingSub,
    SinF32,
    SinF64,
    SimdAdd,
    SimdAnd,
    SimdDiv,
    SimdRem,
    SimdEq,
    SimdExtract,
    SimdGe,
    SimdGt,
    SimdInsert,
    SimdLe,
    SimdLt,
    SimdMul,
    SimdNe,
    SimdOr,
    SimdShl,
    SimdShr,
    SimdShuffle(String),
    SimdSub,
    SimdXor,
    SizeOfVal,
    SqrtF32,
    SqrtF64,
    SubWithOverflow,
    Transmute,
    TruncF32,
    TruncF64,
    TypedSwap,
    UnalignedVolatileLoad,
    UncheckedDiv,
    UncheckedRem,
    Unlikely,
    VolatileCopyMemory,
    VolatileCopyNonOverlappingMemory,
    VolatileLoad,
    VolatileStore,
    VtableSize,
    VtableAlign,
    WrappingAdd,
    WrappingMul,
    WrappingSub,
    WriteBytes,
    Unimplemented { name: String, issue_link: String },
}

/// Assert that top-level types of a function signature match the given patterns.
macro_rules! assert_sig_matches {
    ($sig:expr, $($input_type:pat),* => $output_type:pat) => {
        let inputs = $sig.inputs();
        let output = $sig.output();
        #[allow(unused_mut)]
        let mut index = 0;
        $(
            #[allow(unused_assignments)]
            {
                assert!(matches!(inputs[index].kind(), TyKind::RigidTy($input_type)));
                index += 1;
            }
        )*
        assert!(inputs.len() == index);
        assert!(matches!(output.kind(), TyKind::RigidTy($output_type)));
    }
}

impl Intrinsic {
    /// Create an intrinsic enum from a given intrinsic instance, shallowly validating the argument types.
    pub fn from_instance(intrinsic_instance: &Instance) -> Self {
        let intrinsic_str = intrinsic_instance.intrinsic_name().unwrap();
        let sig = intrinsic_instance.ty().kind().fn_sig().unwrap().skip_binder();
        match intrinsic_str.as_str() {
            "add_with_overflow" => {
                assert_sig_matches!(sig, _, _ => RigidTy::Tuple(_));
                Self::AddWithOverflow
            }
            "align_of" => unreachable!(
                "Expected `core::intrinsics::align_of` to be handled by NullOp::SizeOf"
            ),
            "align_of_val" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not) => RigidTy::Uint(UintTy::Usize));
                Self::AlignOfVal
            }
            "arith_offset" => {
                assert_sig_matches!(sig,
                    RigidTy::RawPtr(_, Mutability::Not),
                    RigidTy::Int(IntTy::Isize)
                    => RigidTy::RawPtr(_, Mutability::Not));
                Self::ArithOffset
            }
            "assert_inhabited" => {
                assert_sig_matches!(sig, => RigidTy::Tuple(_));
                Self::AssertInhabited
            }
            "assert_mem_uninitialized_valid" => {
                assert_sig_matches!(sig, => RigidTy::Tuple(_));
                Self::AssertMemUninitializedValid
            }
            "assert_zero_valid" => {
                assert_sig_matches!(sig, => RigidTy::Tuple(_));
                Self::AssertZeroValid
            }
            "assume" => {
                assert_sig_matches!(sig, RigidTy::Bool => RigidTy::Tuple(_));
                Self::Assume
            }
            "bitreverse" => {
                assert_sig_matches!(sig, _ => _);
                Self::Bitreverse
            }
            "black_box" => {
                assert_sig_matches!(sig, _ => _);
                Self::BlackBox
            }
            "breakpoint" => {
                assert_sig_matches!(sig, => RigidTy::Tuple(_));
                Self::Breakpoint
            }
            "bswap" => {
                assert_sig_matches!(sig, _ => _);
                Self::Bswap
            }
            "caller_location" => {
                assert_sig_matches!(sig, => RigidTy::Ref(_, _, Mutability::Not));
                Self::Unimplemented {
                    name: intrinsic_str,
                    issue_link: "https://github.com/model-checking/kani/issues/374".into(),
                }
            }
            "catch_unwind" => {
                assert_sig_matches!(sig, RigidTy::FnPtr(_), RigidTy::RawPtr(_, Mutability::Mut), RigidTy::FnPtr(_) => RigidTy::Int(IntTy::I32));
                Self::Unimplemented {
                    name: intrinsic_str,
                    issue_link: "https://github.com/model-checking/kani/issues/267".into(),
                }
            }
            "compare_bytes" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not), RigidTy::RawPtr(_, Mutability::Not), RigidTy::Uint(UintTy::Usize) => RigidTy::Int(IntTy::I32));
                Self::CompareBytes
            }
            "copy" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not), RigidTy::RawPtr(_, Mutability::Mut), RigidTy::Uint(UintTy::Usize) => RigidTy::Tuple(_));
                Self::Copy
            }
            "copy_nonoverlapping" => unreachable!(
                "Expected `core::intrinsics::unreachable` to be handled by `StatementKind::CopyNonOverlapping`"
            ),
            "ctlz" => {
                assert_sig_matches!(sig, _ => RigidTy::Uint(UintTy::U32));
                Self::Ctlz
            }
            "ctlz_nonzero" => {
                assert_sig_matches!(sig, _ => RigidTy::Uint(UintTy::U32));
                Self::CtlzNonZero
            }
            "ctpop" => {
                assert_sig_matches!(sig, _ => RigidTy::Uint(UintTy::U32));
                Self::Ctpop
            }
            "cttz" => {
                assert_sig_matches!(sig, _ => RigidTy::Uint(UintTy::U32));
                Self::Cttz
            }
            "cttz_nonzero" => {
                assert_sig_matches!(sig, _ => RigidTy::Uint(UintTy::U32));
                Self::CttzNonZero
            }
            "discriminant_value" => {
                assert_sig_matches!(sig, RigidTy::Ref(_, _, Mutability::Not) => _);
                Self::DiscriminantValue
            }
            "exact_div" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::ExactDiv
            }
            "fadd_fast" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::FaddFast
            }
            "fdiv_fast" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::FdivFast
            }
            "float_to_int_unchecked" => {
                assert_sig_matches!(sig, RigidTy::Float(_) => _);
                assert!(matches!(
                    sig.output().kind(),
                    TyKind::RigidTy(RigidTy::Int(_)) | TyKind::RigidTy(RigidTy::Uint(_))
                ));
                Self::FloatToIntUnchecked
            }
            "fmul_fast" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::FmulFast
            }
            "forget" => {
                assert_sig_matches!(sig, _ => RigidTy::Tuple(_));
                Self::Forget
            }
            "fsub_fast" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::FsubFast
            }
            "is_val_statically_known" => {
                assert_sig_matches!(sig, _ => RigidTy::Bool);
                Self::IsValStaticallyKnown
            }
            "likely" => {
                assert_sig_matches!(sig, RigidTy::Bool => RigidTy::Bool);
                Self::Likely
            }
            "mul_with_overflow" => {
                assert_sig_matches!(sig, _, _ => RigidTy::Tuple(_));
                Self::MulWithOverflow
            }
            // For const eval of nullary intrinsics, see https://github.com/rust-lang/rust/pull/142839
            "needs_drop" => unreachable!(
                "Expected nullary intrinsic `core::intrinsics::type_id` to be const-evaluated before codegen"
            ),
            // As of https://github.com/rust-lang/rust/pull/110822 the `offset` intrinsic is lowered to `mir::BinOp::Offset`
            "offset" => unreachable!(
                "Expected `core::intrinsics::unreachable` to be handled by `BinOp::OffSet`"
            ),
            "ptr_guaranteed_cmp" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not), RigidTy::RawPtr(_, Mutability::Not) => RigidTy::Uint(UintTy::U8));
                Self::PtrGuaranteedCmp
            }
            "ptr_offset_from" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not), RigidTy::RawPtr(_, Mutability::Not) => RigidTy::Int(IntTy::Isize));
                Self::PtrOffsetFrom
            }
            "ptr_offset_from_unsigned" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not), RigidTy::RawPtr(_, Mutability::Not) => RigidTy::Uint(UintTy::Usize));
                Self::PtrOffsetFromUnsigned
            }
            "raw_eq" => {
                assert_sig_matches!(sig, RigidTy::Ref(_, _, Mutability::Not), RigidTy::Ref(_, _, Mutability::Not) => RigidTy::Bool);
                Self::RawEq
            }
            "rotate_left" => {
                assert_sig_matches!(sig, _, RigidTy::Uint(UintTy::U32) => _);
                Self::RotateLeft
            }
            "rotate_right" => {
                assert_sig_matches!(sig, _, RigidTy::Uint(UintTy::U32) => _);
                Self::RotateRight
            }
            "saturating_add" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::SaturatingAdd
            }
            "saturating_sub" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::SaturatingSub
            }
            "size_of" => {
                unreachable!("Expected `core::intrinsics::size_of` to be handled by NullOp::SizeOf")
            }
            "size_of_val" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not) => RigidTy::Uint(UintTy::Usize));
                Self::SizeOfVal
            }
            "sub_with_overflow" => {
                assert_sig_matches!(sig, _, _ => RigidTy::Tuple(_));
                Self::SubWithOverflow
            }
            "transmute" => {
                assert_sig_matches!(sig, _ => _);
                Self::Transmute
            }
            "type_id" => unreachable!(
                "Expected nullary intrinsic `core::intrinsics::type_id` to be const-evaluated before codegen"
            ),
            "type_name" => unreachable!(
                "Expected nullary intrinsic `core::intrinsics::type_name` to be const-evaluated before codegen"
            ),
            "typed_swap_nonoverlapping" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), RigidTy::RawPtr(_, Mutability::Mut) => RigidTy::Tuple(_));
                Self::TypedSwap
            }
            "unaligned_volatile_load" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not) => _);
                Self::UnalignedVolatileLoad
            }
            "unchecked_add" | "unchecked_mul" | "unchecked_shl" | "unchecked_shr"
            | "unchecked_sub" => {
                unreachable!("Expected intrinsic `{intrinsic_str}` to be lowered before codegen")
            }
            "unchecked_div" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::UncheckedDiv
            }
            "unchecked_rem" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::UncheckedRem
            }
            "unlikely" => {
                assert_sig_matches!(sig, RigidTy::Bool => RigidTy::Bool);
                Self::Unlikely
            }
            "unreachable" => unreachable!(
                "Expected `std::intrinsics::unreachable` to be handled by `TerminatorKind::Unreachable`"
            ),
            "variant_count" => unreachable!(
                "Expected nullary intrinsic `core::intrinsics::variant_count` to be const-evaluated before codegen"
            ),
            "volatile_copy_memory" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), RigidTy::RawPtr(_, Mutability::Not), RigidTy::Uint(UintTy::Usize) => RigidTy::Tuple(_));
                Self::VolatileCopyMemory
            }
            "volatile_copy_nonoverlapping_memory" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), RigidTy::RawPtr(_, Mutability::Not), RigidTy::Uint(UintTy::Usize) => RigidTy::Tuple(_));
                Self::VolatileCopyNonOverlappingMemory
            }
            "volatile_load" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not) => _);
                Self::VolatileLoad
            }
            "volatile_store" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => RigidTy::Tuple(_));
                Self::VolatileStore
            }
            "vtable_size" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not) => RigidTy::Uint(UintTy::Usize));
                Self::VtableSize
            }
            "vtable_align" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not) => RigidTy::Uint(UintTy::Usize));
                Self::VtableAlign
            }
            "wrapping_add" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::WrappingAdd
            }
            "wrapping_mul" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::WrappingMul
            }
            "wrapping_sub" => {
                assert_sig_matches!(sig, _, _ => _);
                Self::WrappingSub
            }
            "write_bytes" => {
                assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), RigidTy::Uint(UintTy::U8), RigidTy::Uint(UintTy::Usize) => RigidTy::Tuple(_));
                Self::WriteBytes
            }
            _ => try_match_atomic(intrinsic_instance)
                .or_else(|| try_match_simd(intrinsic_instance))
                .or_else(|| try_match_f32(intrinsic_instance))
                .or_else(|| try_match_f64(intrinsic_instance))
                .unwrap_or(Self::Unimplemented {
                    name: intrinsic_str,
                    issue_link: "https://github.com/model-checking/kani/issues/new/choose".into(),
                }),
        }
    }
}

/// Match atomic intrinsics by instance, returning an instance of the intrinsics enum if the match
/// is successful.
fn try_match_atomic(intrinsic_instance: &Instance) -> Option<Intrinsic> {
    let intrinsic_str = intrinsic_instance.intrinsic_name().unwrap();
    let sig = intrinsic_instance.ty().kind().fn_sig().unwrap().skip_binder();
    match intrinsic_str.as_str() {
        "atomic_and" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => _);
            Some(Intrinsic::AtomicAnd)
        }
        "atomic_cxchgweak" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _, _ => RigidTy::Tuple(_));
            Some(Intrinsic::AtomicCxchgWeak)
        }
        "atomic_cxchg" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _, _ => RigidTy::Tuple(_));
            Some(Intrinsic::AtomicCxchg)
        }
        "atomic_fence" => {
            assert_sig_matches!(sig, => RigidTy::Tuple(_));
            Some(Intrinsic::AtomicFence)
        }
        "atomic_load" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Not) => _);
            Some(Intrinsic::AtomicLoad)
        }
        "atomic_max" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => _);
            Some(Intrinsic::AtomicMax)
        }
        "atomic_min" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => _);
            Some(Intrinsic::AtomicMin)
        }
        "atomic_nand" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => _);
            Some(Intrinsic::AtomicNand)
        }
        "atomic_or" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => _);
            Some(Intrinsic::AtomicOr)
        }
        "atomic_singlethreadfence" => {
            assert_sig_matches!(sig, => RigidTy::Tuple(_));
            Some(Intrinsic::AtomicSingleThreadFence)
        }
        "atomic_store" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => RigidTy::Tuple(_));
            Some(Intrinsic::AtomicStore)
        }
        "atomic_umax" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => _);
            Some(Intrinsic::AtomicUmax)
        }
        "atomic_umin" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => _);
            Some(Intrinsic::AtomicUmin)
        }
        "atomic_xadd" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => _);
            Some(Intrinsic::AtomicXadd)
        }
        "atomic_xchg" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => _);
            Some(Intrinsic::AtomicXchg)
        }
        "atomic_xor" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => _);
            Some(Intrinsic::AtomicXor)
        }
        "atomic_xsub" => {
            assert_sig_matches!(sig, RigidTy::RawPtr(_, Mutability::Mut), _ => _);
            Some(Intrinsic::AtomicXsub)
        }
        _ => None,
    }
}

/// Match SIMD intrinsics by instance, returning an instance of the intrinsics enum if the match
/// is successful.
fn try_match_simd(intrinsic_instance: &Instance) -> Option<Intrinsic> {
    let intrinsic_str = intrinsic_instance.intrinsic_name().unwrap();
    let sig = intrinsic_instance.ty().kind().fn_sig().unwrap().skip_binder();
    match intrinsic_str.as_str() {
        "simd_add" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdAdd)
        }
        "simd_and" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdAnd)
        }
        "simd_div" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdDiv)
        }
        "simd_rem" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdRem)
        }
        "simd_eq" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdEq)
        }
        "simd_extract" => {
            assert_sig_matches!(sig, _, RigidTy::Uint(UintTy::U32) => _);
            Some(Intrinsic::SimdExtract)
        }
        "simd_ge" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdGe)
        }
        "simd_gt" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdGt)
        }
        "simd_insert" => {
            assert_sig_matches!(sig, _, RigidTy::Uint(UintTy::U32), _ => _);
            Some(Intrinsic::SimdInsert)
        }
        "simd_le" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdLe)
        }
        "simd_lt" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdLt)
        }
        "simd_mul" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdMul)
        }
        "simd_ne" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdNe)
        }
        "simd_or" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdOr)
        }
        "simd_shl" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdShl)
        }
        "simd_shr" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdShr)
        }
        "simd_sub" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdSub)
        }
        "simd_xor" => {
            assert_sig_matches!(sig, _, _ => _);
            Some(Intrinsic::SimdXor)
        }
        name => {
            if let Some(suffix) = name.strip_prefix("simd_shuffle") {
                assert_sig_matches!(sig, _, _, _ => _);
                Some(Intrinsic::SimdShuffle(suffix.into()))
            } else {
                None
            }
        }
    }
}

/// Match f32 arithmetic intrinsics by instance, returning an instance of the intrinsics enum if the match
/// is successful.
fn try_match_f32(intrinsic_instance: &Instance) -> Option<Intrinsic> {
    let intrinsic_str = intrinsic_instance.intrinsic_name().unwrap();
    let sig = intrinsic_instance.ty().kind().fn_sig().unwrap().skip_binder();
    match intrinsic_str.as_str() {
        "ceilf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::CeilF32)
        }
        "copysignf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32), RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::CopySignF32)
        }
        "cosf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::CosF32)
        }
        "exp2f32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::Exp2F32)
        }
        "expf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::ExpF32)
        }
        "fabsf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::FabsF32)
        }
        "floorf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::FloorF32)
        }
        "fmaf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32), RigidTy::Float(FloatTy::F32), RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::FmafF32)
        }
        "log10f32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::Log10F32)
        }
        "log2f32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::Log2F32)
        }
        "logf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::LogF32)
        }
        "maxnumf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32), RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::MaxNumF32)
        }
        "minnumf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32), RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::MinNumF32)
        }
        "powf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32), RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::PowF32)
        }
        "powif32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32), RigidTy::Int(IntTy::I32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::PowIF32)
        }
        "roundf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::RoundF32)
        }
        "round_ties_even_f32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::RoundTiesEvenF32)
        }
        "sinf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::SinF32)
        }
        "sqrtf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::SqrtF32)
        }
        "truncf32" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F32) => RigidTy::Float(FloatTy::F32));
            Some(Intrinsic::TruncF32)
        }
        _ => None,
    }
}

/// Match f64 arithmetic intrinsics by instance, returning an instance of the intrinsics enum if the match
/// is successful.
fn try_match_f64(intrinsic_instance: &Instance) -> Option<Intrinsic> {
    let intrinsic_str = intrinsic_instance.intrinsic_name().unwrap();
    let sig = intrinsic_instance.ty().kind().fn_sig().unwrap().skip_binder();
    match intrinsic_str.as_str() {
        "ceilf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::CeilF64)
        }
        "copysignf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64), RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::CopySignF64)
        }
        "cosf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::CosF64)
        }
        "exp2f64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::Exp2F64)
        }
        "expf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::ExpF64)
        }
        "fabsf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::FabsF64)
        }
        "floorf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::FloorF64)
        }
        "fmaf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64), RigidTy::Float(FloatTy::F64), RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::FmafF64)
        }
        "log10f64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::Log10F64)
        }
        "log2f64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::Log2F64)
        }
        "logf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::LogF64)
        }
        "maxnumf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64), RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::MaxNumF64)
        }
        "minnumf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64), RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::MinNumF64)
        }
        "powf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64), RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::PowF64)
        }
        "powif64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64), RigidTy::Int(IntTy::I32) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::PowIF64)
        }
        "roundf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::RoundF64)
        }
        "round_ties_even_f64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::RoundTiesEvenF64)
        }
        "sinf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::SinF64)
        }
        "sqrtf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::SqrtF64)
        }
        "truncf64" => {
            assert_sig_matches!(sig, RigidTy::Float(FloatTy::F64) => RigidTy::Float(FloatTy::F64));
            Some(Intrinsic::TruncF64)
        }
        _ => None,
    }
}
