#![allow(non_snake_case)]

use std::any::{Any, type_name as reflected_name};
use std::thread::spawn as launch;

pub fn rvs_run_ABIMS() {
    launch(|| {});
    let _ = reflected_name::<u8>();
    let value: &dyn Any = &1u8;
    let _ = value.type_id();
}
