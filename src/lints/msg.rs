use rustc_errors::{Diag, DiagCtxtHandle, Diagnostic, Level};
use rustc_span::Span;

#[derive(Debug)]
pub(crate) struct Msg {
    pub span: Span,
    pub text: String,
}

impl Msg {
    pub(crate) fn new(span: Span, text: impl Into<String>) -> Self {
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
