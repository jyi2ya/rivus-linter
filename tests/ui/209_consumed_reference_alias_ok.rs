// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

type Borrowed = &'static mut String;

trait BorrowType {
    type Borrowed;
}

struct Marker;

impl BorrowType for Marker {
    type Borrowed = &'static mut String;
}

fn rvs_process_reference_alias(value: Borrowed) -> Result<(), ()> {
    value.clear();
    Err(())
}

fn rvs_process_reference_projection(
    value: <Marker as BorrowType>::Borrowed,
) -> Result<(), ()> {
    value.clear();
    Err(())
}
