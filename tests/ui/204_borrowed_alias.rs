#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

type Text = String;
type Bytes = Vec<u8>;
type Heap = Box<u8>;

fn rvs_read_text(value: &Text) -> usize {
    value.len()
}

fn rvs_read_bytes(value: &Bytes) -> usize {
    value.len()
}

fn rvs_read_heap(value: &Heap) -> u8 {
    **value
}
