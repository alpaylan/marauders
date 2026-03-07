#[cfg(feature = "syntax-rust-functional")]
pub(crate) mod core;
#[cfg(feature = "syntax-rust-functional")]
mod rust;

#[cfg(feature = "syntax-rust-functional")]
pub(crate) use core::{
    functional_language_for_extension, looks_like_mutations, parse_variations,
    render_comment_code_from_functional, render_functional_code,
};

#[cfg(feature = "syntax-rust-functional")]
pub(crate) use rust::import_rust_mutants_from_files;

#[cfg(not(feature = "syntax-rust-functional"))]
use crate::code::Span;

#[cfg(not(feature = "syntax-rust-functional"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FunctionalLanguage {}

#[cfg(not(feature = "syntax-rust-functional"))]
pub(crate) fn functional_language_for_extension(_extension: &str) -> Option<FunctionalLanguage> {
    None
}

#[cfg(not(feature = "syntax-rust-functional"))]
pub(crate) fn looks_like_mutations(_language: FunctionalLanguage, _input: &str) -> bool {
    false
}

#[cfg(not(feature = "syntax-rust-functional"))]
pub(crate) fn parse_variations(_language: FunctionalLanguage, _input: &str) -> Vec<Span> {
    vec![]
}

#[cfg(not(feature = "syntax-rust-functional"))]
pub(crate) fn render_functional_code(
    _language: FunctionalLanguage,
    _input: &str,
    _spans: &[Span],
) -> anyhow::Result<String> {
    Err(anyhow::anyhow!(
        "Rust functional syntax requires the 'syntax-rust-functional' feature"
    ))
}

#[cfg(not(feature = "syntax-rust-functional"))]
pub(crate) fn render_comment_code_from_functional(
    _language: FunctionalLanguage,
    _input: &str,
) -> anyhow::Result<String> {
    Err(anyhow::anyhow!(
        "Rust functional syntax requires the 'syntax-rust-functional' feature"
    ))
}

#[cfg(not(feature = "syntax-rust-functional"))]
pub(crate) fn import_rust_mutants_from_files(
    _base_source: &str,
    _mutant_sources: &[String],
    _name_prefix: &str,
) -> anyhow::Result<String> {
    Err(anyhow::anyhow!(
        "Rust mutant import requires the 'import-rust-mutants' feature"
    ))
}
