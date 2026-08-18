#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

type Result<T, E> = std::result::Result<T, E>;
type UnitResult<E> = std::result::Result<(), E>;
type FlippedResult<E, T> = std::result::Result<T, E>;

#[derive(Debug)]
enum ParseError {
    Invalid,
}

fn rvs_validate(value: String) -> Result<(), ParseError> {
    drop(value);
    Err(ParseError::Invalid)
}

fn rvs_validate_unit_alias(value: String) -> UnitResult<ParseError> {
    drop(value);
    Err(ParseError::Invalid)
}

fn rvs_validate_flipped_alias(value: String) -> FlippedResult<ParseError, ()> {
    drop(value);
    Err(ParseError::Invalid)
}

async fn rvs_validate_async(value: String) -> Result<(), ParseError> {
    drop(value);
    Err(ParseError::Invalid)
}
