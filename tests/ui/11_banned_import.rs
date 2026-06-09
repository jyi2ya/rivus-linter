// NOTE: RVS_BANNED_IMPORT cannot be tested in UI tests because the banned
// crate is not available as a dependency. The compiler fails with E0432
// before the lint pass runs. This fixture validates the compilation error.
#![expect(non_snake_case)]

use anyhow::Error;

fn main() {}
