// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Checks that the SIMD intrinsics `simd_add`, `simd_sub` and
//! `simd_mul` are supported and return the expected results.
#![feature(repr_simd, core_intrinsics)]
use std::intrinsics::simd::{simd_add, simd_mul, simd_sub};

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct i8x2([i8; 2]);

impl i8x2 {
    fn into_array(self) -> [i8; 2] {
        unsafe { std::mem::transmute(self) }
    }
}

macro_rules! verify_no_overflow {
    ($cf: ident, $uf: ident) => {{
        let a: i8 = kani::any();
        let b: i8 = kani::any();
        let checked = a.$cf(b);
        kani::assume(checked.is_some());
        let simd_a = i8x2([a, a]);
        let simd_b = i8x2([b, b]);
        let unchecked: i8x2 = unsafe { $uf(simd_a, simd_b) };
        assert!(checked.unwrap() == unchecked.into_array()[0]);
        assert!(checked.unwrap() == unchecked.into_array()[1]);
    }};
}

#[kani::proof]
fn test_simd_ops() {
    verify_no_overflow!(checked_add, simd_add);
    verify_no_overflow!(checked_sub, simd_sub);
    verify_no_overflow!(checked_mul, simd_mul);
}
