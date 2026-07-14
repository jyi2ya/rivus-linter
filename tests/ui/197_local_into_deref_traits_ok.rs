// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

trait Into<T> {
    fn rvs_convert(&self) -> T;
}

trait Deref {
    type Target;

    fn rvs_target(&self) -> &Self::Target;
}

#[derive(Debug)]
struct Value(i32);

impl Into<i32> for Value {
    fn rvs_convert(&self) -> i32 {
        self.0
    }
}

impl Deref for Value {
    type Target = i32;

    fn rvs_target(&self) -> &Self::Target {
        &self.0
    }
}

#[test]
fn test_20260714_local_into_deref_traits_ok() {
    let value = Value(7);
    assert_eq!(value.rvs_convert(), *value.rvs_target());
}
