// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Checks that storing the result of a vector comparison in a vector of floats
//! causes an error.
#![feature(repr_simd, core_intrinsics)]
use std::intrinsics::simd::simd_eq;

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct i64x2([i64; 2]);

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct u64x2([u64; 2]);

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct u32x4([u32; 4]);

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct f32x2([f32; 2]);

impl f32x2 {
    fn into_array(self) -> [f32; 2] {
        unsafe { std::mem::transmute(self) }
    }
}

#[kani::proof]
fn main() {
    let x = u64x2([0, 0]);
    let y = u64x2([0, 1]);

    unsafe {
        let invalid_simd: f32x2 = simd_eq(x, y);
        assert!(invalid_simd.into_array() == f32x2([0.0, -1.0]).into_array());
        // ^^^^ The code above fails to type-check in Rust with the error:
        // ```
        // error[E0511]: invalid monomorphization of `simd_eq` intrinsic: expected return type with integer elements, found `f32x2` with non-integer `f32`
        // ```
    }
}
