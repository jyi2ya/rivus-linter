#![allow(non_snake_case)]

#[derive(Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn rvs_shift_M(&mut self, dx: i32, dy: i32) {
        self.x += dx;
        self.y += dy;
    }

    // Explicit receiver spelling is equivalent to `&mut self`.
    pub fn rvs_reset_M(self: &mut Self) {
        self.x = 0;
        self.y = 0;
    }
}
