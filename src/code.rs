use std::fmt::Display;

use crate::{languages::Language, variation::Variation};

#[derive(Debug)]
pub struct Code {
    pub language: Language,
    pub parts: Vec<Span>,
    pub path: String,
}

impl Code {
    pub fn new(language: Language, parts: Vec<Span>, path: String) -> Code {
        Code {
            language,
            parts,
            path,
        }
    }
}

type Constant = String;

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub(crate) line: usize,
    pub(crate) content: SpanContent,
}

impl Span {
    pub(crate) fn constant(content: String, line: usize) -> Span {
        Span {
            line,
            content: SpanContent::Constant(content),
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
    Constant(Constant),
}

impl Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut content = String::new();
        for part in &self.parts {
            match &part.content {
                SpanContent::Constant(c) => content.push_str(c),
                SpanContent::Variation(v) => {
                    content.push('\n');

                    let mut variation_title = String::new();

                    if let Some(name) = &v.name {
                        variation_title.push_str(name);
                        variation_title.push(' ');
                    }

                    if !v.tags.is_empty() {
                        variation_title.push_str(format!("[{}] ", v.tags.join(", ")).as_str());
                    }
                    content.push_str(&self.language.variation_begin(&variation_title));
                    content.push('\n');
                    if v.active == 0 {
                        content.push_str(&v.base);
                    } else {
                        content.push_str(&self.language.variant_body_begin());
                        content.push('\n');
                        content.push_str(&v.base);
                        content.push('\n');
                        content.push_str(&self.language.variant_body_end());
                    }
                    content.push('\n');

                    for (i, variant) in v.variants.iter().enumerate() {
                        content.push_str(&self.language.variant_header_begin());
                        content.push(' ');
                        content.push_str(&variant.name);
                        content.push(' ');
                        content.push_str(&self.language.variant_header_end());
                        content.push('\n');
                        if i + 1 != v.active {
                            content.push_str(&self.language.variant_body_begin());
                            content.push('\n');
                        }
                        content.push_str(&variant.code);
                        content.push('\n');
                        if i + 1 != v.active {
                            content.push_str(&self.language.variant_body_end());
                            content.push('\n');
                        }
                    }
                    content.push_str(&self.language.variation_end());
                    content.push('\n');
                }
            }
        }
        write!(f, "{}", content)
    }
}

impl Code {
    pub(crate) fn from_file(filepath: &str) -> anyhow::Result<Code> {
        // read the file and parse it
        let file_content = std::fs::read_to_string(filepath)?;
        let language = Code::detect_language(filepath);
        let spans = crate::syntax::comment::parse_code(&file_content)?;
        Ok(Code::new(language, spans, filepath.to_string()))
    }

    pub(crate) fn save_to_file(&self, filepath: &str) -> anyhow::Result<()> {
        // write the code to a file
        let content = format!("{}", self);
        std::fs::write(filepath, content)
            .map_err(|e| anyhow::anyhow!("could not write to file: {}", e))
    }

    pub(crate) fn detect_language(filepath: &str) -> Language {
        let ext = filepath.split('.').last().unwrap().to_string();
        Language::extension_to_language(&ext).unwrap()
    }

    pub(crate) fn get_all_variants(&self) -> Vec<String> {
        // get all the variations in the code
        self.parts
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
        match self.parts[variation_index].content {
            SpanContent::Variation(ref mut v) => {
                v.active = variant_index;
            }
            _ => anyhow::bail!("invalid variation index"),
        }

        self.save_to_file(&self.path)
    }
}
