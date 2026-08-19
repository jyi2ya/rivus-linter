// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_good_BMS(buffer: &mut Vec<u8>) {
    buffer.push(1);
    let _ = std::env::var("HOME");
}

#[test]
fn test_20260612_unknown_suffix_no_unknown_ok() {
    rvs_good_BMS(&mut vec![1]);
}
