// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Checks that the following SIMD intrinsics are supported:
//!  * `simd_and`
//!  * `simd_or`
//!  * `simd_xor`
//! This is done by initializing vectors with the contents of 2-member tuples
//! with symbolic values. The result of using each of the intrinsics is compared
//! against the result of using the associated bitwise operator on the tuples.
#![feature(repr_simd, core_intrinsics)]
use std::intrinsics::simd::{simd_and, simd_or, simd_xor};

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct i8x2([i8; 2]);

impl i8x2 {
    fn into_array(self) -> [i8; 2] {
        unsafe { std::mem::transmute(self) }
    }
}

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct i16x2([i16; 2]);

impl i16x2 {
    fn into_array(self) -> [i16; 2] {
        unsafe { std::mem::transmute(self) }
    }
}

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct i32x2([i32; 2]);

impl i32x2 {
    fn into_array(self) -> [i32; 2] {
        unsafe { std::mem::transmute(self) }
    }
}

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct i64x2([i64; 2]);

impl i64x2 {
    fn into_array(self) -> [i64; 2] {
        unsafe { std::mem::transmute(self) }
    }
}

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct u8x2([u8; 2]);

impl u8x2 {
    fn into_array(self) -> [u8; 2] {
        unsafe { std::mem::transmute(self) }
    }
}

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct u16x2([u16; 2]);

impl u16x2 {
    fn into_array(self) -> [u16; 2] {
        unsafe { std::mem::transmute(self) }
    }
}

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct u32x2([u32; 2]);

impl u32x2 {
    fn into_array(self) -> [u32; 2] {
        unsafe { std::mem::transmute(self) }
    }
}

#[repr(simd)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct u64x2([u64; 2]);

impl u64x2 {
    fn into_array(self) -> [u64; 2] {
        unsafe { std::mem::transmute(self) }
    }
}

macro_rules! compare_simd_op_with_normal_op {
    ($simd_op: ident, $normal_op: tt, $simd_type: ident) => {
        let tup_x: (_,_) = kani::any();
        let tup_y: (_,_) = kani::any();
        let x = $simd_type([tup_x.0, tup_x.1]);
        let y = $simd_type([tup_y.0, tup_y.1]);
        let res_and = unsafe { $simd_op(x, y) };
        assert_eq!(tup_x.0 $normal_op tup_y.0, res_and.into_array()[0]);
        assert_eq!(tup_x.1 $normal_op tup_y.1, res_and.into_array()[1]);
    };
}

#[kani::proof]
fn test_simd_and() {
    compare_simd_op_with_normal_op!(simd_and, &, i8x2);
    compare_simd_op_with_normal_op!(simd_and, &, i16x2);
    compare_simd_op_with_normal_op!(simd_and, &, i32x2);
    compare_simd_op_with_normal_op!(simd_and, &, i64x2);
    compare_simd_op_with_normal_op!(simd_and, &, u8x2);
    compare_simd_op_with_normal_op!(simd_and, &, u16x2);
    compare_simd_op_with_normal_op!(simd_and, &, u32x2);
    compare_simd_op_with_normal_op!(simd_and, &, u64x2);
}

#[kani::proof]
fn test_simd_or() {
    compare_simd_op_with_normal_op!(simd_or, |, i8x2);
    compare_simd_op_with_normal_op!(simd_or, |, i16x2);
    compare_simd_op_with_normal_op!(simd_or, |, i32x2);
    compare_simd_op_with_normal_op!(simd_or, |, i64x2);
    compare_simd_op_with_normal_op!(simd_or, |, u8x2);
    compare_simd_op_with_normal_op!(simd_or, |, u16x2);
    compare_simd_op_with_normal_op!(simd_or, |, u32x2);
    compare_simd_op_with_normal_op!(simd_or, |, u64x2);
}

#[kani::proof]
fn test_simd_xor() {
    compare_simd_op_with_normal_op!(simd_xor, ^, i8x2);
    compare_simd_op_with_normal_op!(simd_xor, ^, i16x2);
    compare_simd_op_with_normal_op!(simd_xor, ^, i32x2);
    compare_simd_op_with_normal_op!(simd_xor, ^, i64x2);
    compare_simd_op_with_normal_op!(simd_xor, ^, u8x2);
    compare_simd_op_with_normal_op!(simd_xor, ^, u16x2);
    compare_simd_op_with_normal_op!(simd_xor, ^, u32x2);
    compare_simd_op_with_normal_op!(simd_xor, ^, u64x2);
}
