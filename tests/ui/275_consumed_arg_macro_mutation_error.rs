#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

macro_rules! propagate {
    ($result:ident) => {{
        $result = Err(1);
        Ok::<(), u8>({
            $result?;
        })?;
    }};
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    propagate!(result);
    Ok(())
}
