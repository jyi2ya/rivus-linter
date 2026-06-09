#![expect(non_snake_case)]

fn rvs_add() {}

fn rvs_pure_calls_panic_P() {
    rvs_add();
    panic!("oops");
}
