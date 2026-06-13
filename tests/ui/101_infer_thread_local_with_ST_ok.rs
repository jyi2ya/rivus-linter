// check-pass
#![allow(non_snake_case)]
#![feature(thread_local)]

#[thread_local]
static TLS: i32 = 42;

fn rvs_read_tls_ST() -> i32 {
    TLS
}
