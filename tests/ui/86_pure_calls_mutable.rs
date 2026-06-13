#![allow(non_snake_case)]

fn rvs_sort_M(arr: &mut [i32]) {
    arr.sort();
}

fn rvs_pure() {
    rvs_sort_M(&mut [3, 1, 2]);
}
