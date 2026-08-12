// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![feature(specialization)]
#![register_tool(rivus)]
#![allow(incomplete_features)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

trait Specialized {
    fn rvs_value() -> u8;
}

impl<T: Clone> Specialized for T {
    default fn rvs_value() -> u8 {
        1
    }
}

impl<T: Copy> Specialized for T {
    fn rvs_value() -> u8 {
        2
    }
}

#[test]
fn test_20260731_specialized_impl_predicate_identity() {
    assert_eq!(<String as Specialized>::rvs_value(), 1);
    assert_eq!(<u8 as Specialized>::rvs_value(), 2);
}
