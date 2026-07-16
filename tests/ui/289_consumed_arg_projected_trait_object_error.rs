#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    let mut set_error = || result = Err(1);
    let mut callbacks: [Option<&mut dyn FnMut()>; 1] = [None];
    callbacks[0] = Some(&mut set_error);
    let [callback] = callbacks;
    callback.expect("present")();
    result
}
