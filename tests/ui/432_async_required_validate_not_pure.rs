// check-pass
// Tests that an async required trait method named rvs_check is not treated
// as pure by the validate lint: A comes from the signature, not the name.
#![allow(non_snake_case)]

trait Parser {
    async fn rvs_check(&self, raw: String) -> Result<(), ParseError>;
}

struct LenientParser;

impl Parser for LenientParser {
    async fn rvs_check(&self, raw: String) -> Result<(), ParseError> {
        if raw.is_empty() {
            Err(ParseError)
        } else {
            Ok(())
        }
    }
}

struct ParseError;

fn main() {}
