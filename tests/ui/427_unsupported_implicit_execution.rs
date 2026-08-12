// compile-flags: --test
#![feature(fn_traits)]
#![feature(register_tool)]
#![feature(unboxed_closures)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![deny(rivus::rvs_unsupported_implicit_execution)]

use core::ops::{Add, Index};

#[derive(Clone, Copy)]
struct Number(u8);

impl Add for Number {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

struct Values([u8; 1]);

impl Index<usize> for Values {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

struct Resource;

impl Drop for Resource {
    fn drop(&mut self) {}
}

fn rvs_overloaded_operator() {
    let _ = Number(1) + Number(2);
}

fn rvs_overloaded_index() {
    let values = Values([1]);
    let _ = values[0];
}

fn rvs_explicit_fn_trait_call() {
    let callback = || 1;
    let _ = FnOnce::call_once(callback, ());
    let callback = || 2;
    let _ = callback.call_once(());
}

fn rvs_inline_asm() {
    unsafe {
        core::arch::asm!("nop");
    }
}

fn rvs_custom_drop() {
    let _resource = Resource;
}

#[test]
fn test_20260811_unsupported_implicit_execution() {
    rvs_overloaded_operator();
    rvs_overloaded_index();
    rvs_explicit_fn_trait_call();
    rvs_inline_asm();
    rvs_custom_drop();
}
