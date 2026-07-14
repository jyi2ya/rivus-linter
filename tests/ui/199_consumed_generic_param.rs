#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[derive(Debug)]
enum WriteError {
    Failed,
}

fn rvs_write<W>(writer: W) -> Result<(), WriteError> {
    drop(writer);
    if std::hint::black_box(false) {
        Err(WriteError::Failed)
    } else {
        Ok(())
    }
}
