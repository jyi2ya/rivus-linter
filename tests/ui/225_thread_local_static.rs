#![feature(thread_local)]
#![allow(non_snake_case)]

#[thread_local]
static TLS_VALUE: i32 = 1;

fn rvs_read_tls_S() -> i32 {
    TLS_VALUE
}
