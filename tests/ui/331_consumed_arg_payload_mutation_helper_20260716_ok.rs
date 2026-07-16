// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[inline(never)]
fn rvs_append_M(result: &mut Result<String, u8>) {
    if let Ok(value) = result.as_mut() {
        value.push('x');
    }
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(String::new());
    rvs_append_M(&mut result);
    match result {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}
