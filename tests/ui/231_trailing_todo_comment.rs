#![allow(non_snake_case)]

fn rvs_trailing_todo_S() {
    let _ = 1; // TODO: replace the placeholder.
}

fn rvs_multiline_fixme_S() {
    /* This comment continues over lines.
     * FIXME: replace the placeholder.
     */
    let _ = 1;
}
