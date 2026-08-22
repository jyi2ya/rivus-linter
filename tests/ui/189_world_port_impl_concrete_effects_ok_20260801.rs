// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]
#![allow(rivus::rvs_untested_ok_fn)]

use std::cell::Cell;
use std::time::Duration;

static GLOBAL_VALUE: i32 = 1;

thread_local! {
    static LOCAL_VALUE: Cell<i32> = const { Cell::new(1) };
}

fn rvs_block_B() -> i32 {
    std::thread::sleep(Duration::ZERO);
    1
}

fn rvs_io_BIS() -> i32 {
    match std::fs::metadata(".") {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

fn rvs_effect_S() -> i32 {
    GLOBAL_VALUE
}

fn rvs_thread_local_ST() -> i32 {
    LOCAL_VALUE.get()
}

trait FetchApi {
    type World;

    fn rvs_fetch_P(world: &mut Self::World) -> i32;
}

#[derive(Debug)]
struct Api;

#[derive(Debug)]
struct ApiWorld;

impl FetchApi for Api {
    type World = ApiWorld;

    fn rvs_fetch_P(_world: &mut Self::World) -> i32 {
        rvs_block_B() + rvs_io_BIS() + rvs_effect_S() + rvs_thread_local_ST()
    }
}

fn rvs_fetch_through_port_MP<E: FetchApi>(world: &mut E::World) -> i32 {
    E::rvs_fetch_P(world)
}
