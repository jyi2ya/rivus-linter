#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

trait UserRepository {
    fn find_by_id(&self, id: u64);
}
