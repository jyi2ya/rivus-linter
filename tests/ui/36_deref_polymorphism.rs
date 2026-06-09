#![expect(non_snake_case)]

struct Wrapper(String);

impl std::ops::Deref for Wrapper {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
