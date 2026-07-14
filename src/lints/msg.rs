use rustc_errors::{Diag, DiagCtxtHandle, Diagnostic, Level};
use rustc_span::Span;

#[derive(Debug)]
pub(crate) struct Msg {
    pub span: Span,
    pub text: String,
}

impl Msg {
    pub(crate) fn rvs_new(span: Span, text: impl Into<String>) -> Self {
        Self {
            span,
            text: text.into(),
        }
    }
}

impl<'a> Diagnostic<'a, ()> for Msg {
    fn into_diag(self, dcx: DiagCtxtHandle<'a>, level: Level) -> Diag<'a, ()> {
        let mut d = Diag::new(dcx, level, format!("{}", self.text));
        d.span(self.span);
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;
    use rustc_span::DUMMY_SP;

    #[test]
    fn test_20260714_msg_constructor_preserves_fields() {
        let message = Msg::rvs_new(DUMMY_SP, "message");
        let output = format!(
            "text={}\nspan_dummy={}\n",
            message.text,
            message.span == DUMMY_SP,
        );
        rvs_snapshot_BIS("test_20260714_msg_constructor_preserves_fields", &output);

        assert_eq!(message.text, "message");
        assert_eq!(message.span, DUMMY_SP);
    }
}
