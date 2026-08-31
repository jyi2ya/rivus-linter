// check-pass
// compile-flags: --test
// Regression for the coverage-candidate registration scope: helpers inside
// `mod tests` are test source and must not become good/ok coverage
// candidates (the offline engine excludes them via is_coverage_candidate).
#![allow(non_snake_case)]

fn rvs_used_by_test() -> u8 {
    1
}

#[cfg(test)]
mod tests {
    fn rvs_test_helper_unused() -> u8 {
        2
    }

    #[test]
    fn test_20260831_test_module_helper_not_coverage_candidate() {
        assert_eq!(super::rvs_used_by_test(), 1);
    }
}
