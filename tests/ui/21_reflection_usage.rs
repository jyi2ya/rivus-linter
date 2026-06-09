#![expect(non_snake_case)]

use std::any::Any;

pub fn rvs_reflect(val: &dyn Any) {
    let _ = val.type_id();
    let _ = std::any::type_name::<i32>();
}
