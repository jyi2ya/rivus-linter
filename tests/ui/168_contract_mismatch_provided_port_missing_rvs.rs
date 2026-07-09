#![feature(register_tool)]
#![register_tool(rivus)]

trait ApiClient {
    fn fetch(&self) -> i32 {
        1
    }
}

fn main() {}
