use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

use anyhow::Context;

use crate::{
    languages::{CustomLanguage, Language},
    variation::Variation,
};

#[derive(Debug)]
pub struct Code {
    pub language: Language,
    pub spans: Vec<Span>,
    pub path: PathBuf,
}

impl Code {
    pub fn new(language: Language, spans: Vec<Span>, path: PathBuf) -> Code {
        Code {
            language,
            spans,
            path,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub(crate) line: usize,
    pub(crate) content: SpanContent,
}

impl Span {
    pub(crate) fn constant(content: String, line: usize) -> Span {
        Span {
            line,
            content: SpanContent::Line(content),
        }
    }

    pub(crate) fn variation(variation: Variation, line: usize) -> Span {
        Span {
            line,
            content: SpanContent::Variation(variation),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpanContent {
    Variation(Variation),
    Line(String),
}

impl Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut content = String::new();
        for part in &self.spans {
            match &part.content {
                SpanContent::Line(c) => content.push_str(c),
                SpanContent::Variation(v) => {
                    let mut variation_title = String::new();

                    if let Some(name) = &v.name {
                        variation_title.push_str(name);
                        variation_title.push(' ');
                    }

                    if !v.tags.is_empty() {
                        variation_title.push_str(format!("[{}] ", v.tags.join(", ")).as_str());
                    }
                    content.push_str(&v.indentation);
                    content.push_str(&self.language.variation_begin(&variation_title));
                    content.push('\n');
                    if v.active == 0 {
                        for line in &v.base.lines() {
                            content.push_str(line);
                            content.push('\n');
                        }
                    } else {
                        let indentation = v.base.indentation().unwrap_or(v.indentation.clone());
                        content.push_str(&indentation);
                        content.push_str(&self.language.variant_body_begin());
                        content.push('\n');
                        for line in &v.base.lines() {
                            content.push_str(line);
                            content.push('\n');
                        }
                        content.push_str(&indentation);
                        content.push_str(&self.language.variant_body_end());
                        content.push('\n');
                    }

                    for (i, variant) in v.variants.iter().enumerate() {
                        let indentation = variant.indentation().unwrap_or(v.indentation.clone());
                        content.push_str(&v.indentation);
                        content.push_str(&self.language.variant_header_begin());
                        content.push(' ');
                        content.push_str(&variant.name);
                        content.push(' ');
                        content.push_str(&self.language.variant_header_end());
                        content.push('\n');
                        if !matches!(variant.body, crate::variation::VariantBody::Active { .. }) {
                            content.push_str(&v.indentation);
                            content.push_str(&self.language.variant_body_begin());
                            content.push('\n');
                        }
                        for line in &variant.lines() {
                            content.push_str(line);
                            content.push('\n');
                        }
                        if !matches!(variant.body, crate::variation::VariantBody::Active { .. }) {
                            content.push_str(&v.indentation);
                            content.push_str(&self.language.variant_body_end());
                            content.push('\n');
                        }
                    }
                    content.push_str(&v.indentation);
                    content.push_str(&self.language.variation_end());
                    content.push('\n');
                }
            }
        }
        write!(f, "{}", content)
    }
}

impl Code {
    pub(crate) fn from_file(
        filepath: &Path,
        custom_languages: &Vec<CustomLanguage>,
    ) -> anyhow::Result<Code> {
        // read the file and parse it
        let file_content = std::fs::read_to_string(filepath)?;
        let extension = filepath.extension().context(format!(
            "file extension is not recognized for '{}'",
            filepath.to_string_lossy()
        ))?;

        // todo: add support for custom languages
        let language = Language::extension_to_language(
            extension.to_str().context(format!(
                "extension is not valid unicode {}",
                extension.to_string_lossy()
            ))?,
            custom_languages,
        );
        log::trace!(
            "detected language for '{}': {}",
            filepath.to_string_lossy(),
            language.as_ref().map_or("unknown", |l| l.file_extension())
        );

        if language.is_none() {
            anyhow::bail!(
                "unsupported file extension '{}'",
                filepath.extension().unwrap().to_str().unwrap()
            );
        }
        let language = language.unwrap();

        let spans = crate::syntax::comment::parse_code(&file_content)?;
        log::debug!(
            "parsed {} spans from file '{}'",
            spans.len(),
            filepath.to_string_lossy()
        );
        log::trace!("spans: {:#?}", spans);
        Ok(Code::new(language, spans, filepath.to_path_buf()))
    }

    pub(crate) fn save_to_file(&self, filepath: &Path) -> anyhow::Result<()> {
        // write the code to a file
        let content = format!("{}", self);
        std::fs::write(filepath, content)
            .map_err(|e| anyhow::anyhow!("could not write to file: {}", e))
    }

    pub(crate) fn detect_language(filepath: &str) -> Language {
        let ext = filepath.split('.').last().unwrap().to_string();
        // todo: add support for custom languages
        Language::extension_to_language(&ext, &vec![]).unwrap()
    }

    pub(crate) fn get_all_variants(&self) -> Vec<String> {
        // get all the variations in the code
        self.spans
            .iter()
            .filter_map(|part| match &part.content {
                SpanContent::Variation(v) => {
                    let variants: Vec<String> = v.variants.iter().map(|v| v.name.clone()).collect();
                    Some(variants)
                }
                _ => None,
            })
            .flatten()
            .collect()
    }
}

impl Code {
    fn longest_common_prefix(strs: &[String]) -> String {
        // Credits for this function goes to
        // https://users.rust-lang.org/t/is-this-code-idiomatic/51798/14
        for (idx, c) in strs[0].char_indices() {
            // Because `s[..idx]` represents a common prefix,
            // `idx` must be a valid character boundary in all the strings
            if !strs[1..].iter().all(|s| s[idx..].starts_with(c)) {
                return strs[0][..idx].to_string();
            }
        }
        strs[0].to_string()
    }

    pub(crate) fn set_active_variant(
        &mut self,
        variation_index: usize,
        variant_index: usize,
    ) -> anyhow::Result<()> {
        // set the active variant of a variation
        log::info!(
            "setting active variant '{}' for variation '{}'",
            variant_index,
            variation_index
        );
        match self.spans[variation_index].content {
            SpanContent::Variation(ref mut v) => {
                log::debug!("variants: {:?}", v.variants);

                let variant = if variant_index == 0 {
                    &mut v.base
                } else {
                    &mut v
                        .variants
                        .get_mut(variant_index - 1)
                        .context("invalid variant index")?
                };

                if variant.is_active() {
                    anyhow::bail!("variant is already active");
                } else {
                    // deactivate the currently active variant
                    v.activate_variant(variant_index);
                }
            }
            _ => anyhow::bail!("invalid variation index"),
        }

        self.save_to_file(&self.path)
    }
}
