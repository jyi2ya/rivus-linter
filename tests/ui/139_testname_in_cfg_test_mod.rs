// compile-flags: --test
#![allow(non_snake_case)]

#[cfg(test)]
mod tests {
    #[test]
    fn stale_name() {}

    #[test]
    fn test_20260612_good_name() {}
}
