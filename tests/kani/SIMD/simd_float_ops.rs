// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Ensure we can handle SIMD defined in the standard library
#![allow(non_camel_case_types)]
#![feature(repr_simd, core_intrinsics, portable_simd)]
use std::intrinsics::simd::simd_add;
use std::simd::f32x4;

#[repr(simd)]
#[derive(Copy, Clone, kani::Arbitrary)]
pub struct f32x2([f32; 2]);

impl f32x2 {
    fn as_array(&self) -> &[f32; 2] {
        unsafe { &*(self as *const f32x2 as *const [f32; 2]) }
    }
}

#[kani::proof]
fn check_sum() {
    let a = f32x2([0.0, 0.0]);
    let b = kani::any::<f32x2>();
    kani::assume(b.as_array()[0].is_normal());
    kani::assume(b.as_array()[1].is_normal());
    let sum = unsafe { simd_add(a, b) };
    assert_eq!(sum.as_array(), b.as_array());
}

#[kani::proof]
fn check_sum_portable() {
    let a = f32x4::splat(0.0);
    let b = f32x4::from_array(kani::any());
    kani::assume(b.as_array()[0].is_normal());
    kani::assume(b.as_array()[1].is_normal());
    kani::assume(b.as_array()[2].is_normal());
    kani::assume(b.as_array()[3].is_normal());
    // Cannot compare them directly: https://github.com/model-checking/kani/issues/2632
    assert_eq!((a + b).as_array(), b.as_array());
}
