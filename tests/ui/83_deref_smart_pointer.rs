#![allow(non_snake_case)]

struct MyBox<T>(T);
impl<T> std::ops::Deref for MyBox<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
