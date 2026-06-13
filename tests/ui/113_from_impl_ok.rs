// check-pass
#![allow(non_snake_case)]

#[derive(Debug)]
struct Celsius(f64);
impl From<Celsius> for f64 {
    fn from(c: Celsius) -> f64 {
        c.0
    }
}
