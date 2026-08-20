#![allow(non_snake_case)]

#[derive(Debug)]
pub struct Report {
    pub title: String,
}

impl Report {
    pub fn rvs_title(&self) -> &String {
        &self.title
    }

    pub fn rvs_line_count(&self) -> usize {
        self.title.len()
    }
}

// A `pub(crate)` field with a `pub(crate)` method is redundant.
#[derive(Debug)]
pub struct Note {
    pub(crate) body: String,
}

impl Note {
    pub(crate) fn rvs_body(&self) -> &String {
        return &self.body;
    }

    // Tail `return` without semicolon is the same projection.
    pub(crate) fn rvs_body_tail(&self) -> &String {
        return &self.body
    }

    // Explicit immutable receiver spelling is the same projection.
    pub(crate) fn rvs_body_explicit(self: &Self) -> &String {
        &self.body
    }
}

// A `pub` method in a private module does not widen access: its effective
// visibility is module-local, so the accessor stays redundant for every
// actual caller.
mod hidden {
    #[derive(Debug)]
    pub struct Inner {
        pub(crate) value: u32,
    }

    impl Inner {
        pub fn rvs_value(&self) -> u32 {
            self.value
        }
    }
}

// Whitespace inside the annotation is still a crate-wide spelling; this
// proves the full vis_span -> snippet -> tokenizer -> classification path.
// The method is crate-visible too, so the projection is redundant.
#[derive(Debug)]
pub struct Spaced {
    pub (crate) data: u32,
}

impl Spaced {
    pub(crate) fn rvs_data(&self) -> u32 {
        self.data
    }
}
