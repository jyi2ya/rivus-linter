// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]
#![allow(rivus::rvs_untested_ok_fn)]

#[derive(Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    /// Computes a derived value instead of projecting a field.
    pub fn rvs_manhattan(&self) -> i32 {
        self.x.abs() + self.y.abs()
    }

    /// Associated constructor — the `Type::load_from` idiom stays allowed.
    pub fn rvs_origin() -> Self {
        Point { x: 0, y: 0 }
    }

    /// By-value `mut self` consumes the struct — not an `&mut self` method.
    pub fn rvs_normalized(mut self) -> Self {
        let _ = &mut self.x;
        self
    }

    /// By-value `self` consumer (builder finish idiom).
    pub fn rvs_into_tuple(self) -> (i32, i32) {
        (self.x, self.y)
    }

    /// Explicit `self: &Self` receiver that computes, not projects.
    pub fn rvs_sum(self: &Self) -> i32 {
        self.x + self.y
    }
}

// A `pub(crate)` field exposed through a `pub` method of a `pub` module
// genuinely widens access for external callers — not redundant.
pub mod widening {
    #[derive(Debug)]
    pub struct Config {
        pub(crate) name: String,
    }

    impl Config {
        /// Widens a crate-visible field to external callers — not redundant.
        pub fn rvs_name(&self) -> &String {
            &self.name
        }
    }
}

// A re-exported type in a private module is externally nameable: the `pub`
// method genuinely widens access even though the defining module is private.
mod reexported {
    #[derive(Debug)]
    pub struct Reexported {
        pub(crate) value: u32,
    }

    impl Reexported {
        /// Accessible through the crate-level re-export — widens access.
        pub fn rvs_value(&self) -> u32 {
            self.value
        }
    }
}

pub use reexported::Reexported;

// Objects with module-local field annotations keep their mutators, even when
// the annotation happens to resolve to the crate root.
mod counters {
    #[derive(Debug)]
    pub struct Counter {
        pub(self) count: u32,
    }

    impl Counter {
        /// `pub(self)` keeps the struct an object.
        pub fn rvs_increment_M(&mut self) {
            self.count += 1;
        }
    }

    #[derive(Debug)]
    pub struct CrateRooted {
        pub(super) value: u32,
    }

    impl CrateRooted {
        /// `pub(super)` resolving to crate root still keeps the object.
        pub fn rvs_bump_M(&mut self) {
            self.value += 1;
        }
    }
}

// Trait impls are exempt.
trait Describe {
    fn rvs_describe(&self) -> String;
}

impl Describe for Point {
    fn rvs_describe(&self) -> String {
        format!("{},{}", self.x, self.y)
    }
}

// Mixed visibility tuple struct is not pure data.
mod pairs {
    mod inner {
        #[derive(Debug)]
        pub struct Pair(pub i32, pub(super) i32);

        impl Pair {
            /// Mixed visibility tuple struct — not pure data.
            pub fn rvs_second(&self) -> i32 {
                self.1
            }
        }
    }

    pub use inner::Pair;
}

// Zero-field struct is never classified as data.
#[derive(Debug)]
pub struct Marker;

impl Marker {
    /// Zero-field structs are never pure data; mutators are allowed.
    pub fn rvs_touch_M(&mut self) {
        let _ = self;
    }
}

// Whitespace inside the annotation is still a crate-wide spelling: this
// struct IS pure data, but its method computes, so no diagnostics fire.
#[derive(Debug)]
pub struct SpacedOk {
    pub (crate) value: u32,
}

impl SpacedOk {
    /// Computes a derived value from the crate-visible field.
    pub fn rvs_doubled(&self) -> u64 {
        u64::from(self.value) * 2
    }
}
