// check-pass
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, rivus::rvs_untested_ok_fn)]

fn rvs_same() {}

mod a {
    pub(super) fn rvs_same() {}
}

mod b {
    pub(super) fn rvs_same() {}
}
