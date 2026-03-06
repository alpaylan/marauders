use crate::code::Span;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FunctionalLanguage {
    Rust,
}

pub(crate) fn functional_language_for_extension(extension: &str) -> Option<FunctionalLanguage> {
    match extension {
        "rs" => Some(FunctionalLanguage::Rust),
        _ => None,
    }
}

pub(crate) fn looks_like_mutations(language: FunctionalLanguage, input: &str) -> bool {
    match language {
        FunctionalLanguage::Rust => super::rust::looks_like_rust_mutations(input),
    }
}

pub(crate) fn parse_variations(language: FunctionalLanguage, input: &str) -> Vec<Span> {
    match language {
        FunctionalLanguage::Rust => super::rust::parse_rust_variations(input),
    }
}

pub(crate) fn render_functional_code(
    language: FunctionalLanguage,
    input: &str,
    spans: &[Span],
) -> anyhow::Result<String> {
    match language {
        FunctionalLanguage::Rust => super::rust::render_rust_functional_code(input, spans),
    }
}

pub(crate) fn render_comment_code_from_functional(
    language: FunctionalLanguage,
    input: &str,
) -> anyhow::Result<String> {
    match language {
        FunctionalLanguage::Rust => super::rust::render_rust_comment_code_from_functional(input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_extension_dispatch() {
        let language = functional_language_for_extension("rs").unwrap();
        assert!(looks_like_mutations(
            language,
            r#"fn x() { let _ = std::env::var("M_demo"); }"#
        ));
    }
}
