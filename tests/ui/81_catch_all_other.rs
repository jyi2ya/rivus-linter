#![allow(non_snake_case)]

enum RepoError {
    Io(std::io::Error),
    Other(String),
}
