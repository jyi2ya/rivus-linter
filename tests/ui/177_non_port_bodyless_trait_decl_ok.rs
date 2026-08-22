// check-pass
#![allow(non_snake_case)]

/// A bodyless trait declaration with no implementations has an empty vote
/// lower bound; the canonical name is the pure form.
trait Handler {
    fn rvs_read(&self);
}
