#![allow(non_snake_case)]

fn rvs_process(name: String) -> Result<(), std::io::Error> {
    drop(name);
    Err(std::io::Error::other("failed"))
}
