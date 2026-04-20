use std::collections::HashMap;

fn rvs_add(x: i32, y: i32) -> i32 {
    x + y
}

#[allow(non_snake_case)]
fn rvs_read_BI(path: &str) -> Result<String, std::io::Error> {
    std::fs::read_to_string(path)
}

#[allow(non_snake_case)]
fn rvs_process_BI(data: &str) -> Result<Vec<i32>, std::io::Error> {
    let map: HashMap<String, i32> = HashMap::new();
    let nums: Vec<i32> = data.lines().map(|s| s.len() as i32).collect();
    Ok(nums)
}

fn main() {
    let _ = rvs_add(1, 2);
    let _ = rvs_read_BI("test.txt");
    let _ = rvs_process_BI("hello\nworld");
}
