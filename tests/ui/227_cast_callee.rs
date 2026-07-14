#![allow(non_snake_case)]

fn rvs_blocking_BI() {
    let _ = 1;
}

fn rvs_cast_call() {
    (rvs_blocking_BI as fn())();
}

fn rvs_cast_drop_S() {
    let result: Result<(), String> = Err(String::new());
    (drop::<Result<(), String>> as fn(Result<(), String>))(result);
}
