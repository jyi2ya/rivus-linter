#![allow(non_snake_case)]

struct Celsius(f64);
impl Into<f64> for Celsius {
    fn into(self) -> f64 {
        self.0
    }
}
