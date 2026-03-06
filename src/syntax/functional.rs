pub(crate) mod core;
mod rust;

pub(crate) use core::{
    functional_language_for_extension, looks_like_mutations, parse_variations,
    render_comment_code_from_functional, render_functional_code,
};

pub(crate) use rust::import_rust_mutants_from_files;
