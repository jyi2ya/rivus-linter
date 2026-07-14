#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[derive(Debug)]
enum ParseError {
    Invalid,
}

trait Outcome {
    type Return;
}

struct Marker;

impl Outcome for Marker {
    type Return = Result<(), ParseError>;
}

fn rvs_validate_projection(value: String) -> <Marker as Outcome>::Return {
    drop(value);
    Err(ParseError::Invalid)
}
