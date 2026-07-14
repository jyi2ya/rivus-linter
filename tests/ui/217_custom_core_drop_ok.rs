// check-pass
// compile-flags: --test --crate-name=core
#![feature(register_tool)]
#![allow(non_snake_case)]
#![register_tool(rivus)]
#![allow(rivus::rvs_non_rvs_fn)]

mod mem {
    pub fn drop<T>(_value: T) {}
}

mod result {
    #[derive(Debug)]
    pub enum Result<T, E> {
        Ok(T),
        Err(E),
    }

    impl<T, E> Result<T, E> {
        pub fn ok(self) -> Option<T> {
            match self {
                Self::Ok(value) => Some(value),
                Self::Err(_) => None,
            }
        }

        pub fn unwrap_or_default(self) -> T
        where
            T: Default,
        {
            match self {
                Self::Ok(value) => value,
                Self::Err(_) => T::default(),
            }
        }
    }
}

fn rvs_custom_core_drop_ok() {
    crate::mem::drop(std::result::Result::<(), ()>::Ok(()));
    let _ = crate::result::Result::<(), ()>::Ok(()).ok();
    let _ = crate::result::Result::<(), ()>::Err(()).unwrap_or_default();
}

#[test]
fn test_20260714_custom_core_drop_ok() {
    rvs_custom_core_drop_ok();
}
