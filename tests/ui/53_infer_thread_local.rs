#![allow(non_snake_case)]

#[thread_local]
static TLS: i32 = 42;

fn rvs_read_tls_T() -> i32 {
    TLS
}
