#![allow(non_snake_case)]

fn rvs_process(data: String) -> Result<(), std::io::Error> {
    drop(data);
    Err(std::io::Error::other("failed"))
}
