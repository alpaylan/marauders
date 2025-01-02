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
        let spans = crate::parser::parse_code(&file_content)?;
        Ok(Code::new(language, spans, filepath.to_string()))
    }

    pub(crate) fn into_file(&self, filepath: &str) -> anyhow::Result<()> {
        // write the code to a file
        let content = format!("{}", self);
        std::fs::write(filepath, content)
            .map_err(|e| anyhow::anyhow!("could not write to file: {}", e))
    }

    pub(crate) fn detect_language(filepath: &str) -> Language {
        let ext = filepath.split('.').last().unwrap().to_string();
        Language::extension_to_language(&ext).unwrap()
    }

    fn get_all_variant_headers(
        variation_content: &str,
        language: &Language,
    ) -> Vec<(String, usize, usize)> {
        let mut headers = Vec::new();
        let mut start = 0;
        // get variant name
        loop {
            let header_begin = variation_content[start..].find(&language.variant_header_begin());

            if header_begin.is_none() {
                break;
            }

            let header_begin = header_begin.unwrap() + start;

            let header_end = variation_content[header_begin..].find(&language.variant_header_end());
            if header_end.is_none() {
                break;
            }

            let header_end = header_end.unwrap() + header_begin;

            let variant_name = variation_content[(header_begin
                + language.variant_header_begin().len())
                ..(header_end - language.variant_header_end().len() + 1)]
                .split_whitespace()
                .next();

            if variant_name.is_none() {
                break;
            }

            headers.push((variant_name.unwrap().to_string(), header_begin, header_end));
            start = header_end;
        }

        headers
    }

    fn remove_comments(variant_content: &str, language: &Language) -> (String, bool) {
        let variant_content = variant_content.trim();
        let is_commented = variant_content.starts_with(&language.comment_begin())
            && variant_content.ends_with(&language.comment_end());
        if is_commented {
            let variant_content =
                variant_content.trim_start_matches(&language.variant_body_begin());
            let variant_content = variant_content.trim_end_matches(&language.variant_body_end());
            (variant_content.trim().to_string(), true)
        } else {
            (variant_content.trim().to_string(), false)
        }
    }

    // fn parse_variation(variation_content: &str, language: &Language) -> Variation {
    //     // parse the variation content and return a variation
    //     let variation_content = variation_content.trim();
    //     let all_variants = Code::get_all_variant_headers(variation_content, language);

    //     if all_variants.is_empty() {
    //         panic!("Variation must have at least a variant");
    //     }

    //     let base = variation_content[..all_variants[0].1].to_string();
    //     let (base, is_base_inactive) = Code::remove_comments(&base, language);
    //     let mut variants = Vec::new();
    //     let mut active = if is_base_inactive { None } else { Some(0) };

    //     for i in 0..all_variants.len() - 1 {
    //         let code = &variation_content
    //             [(all_variants[i].2 + language.variant_header_end().len())..all_variants[i + 1].1];

    //         let (code, inactive) = Code::remove_comments(code, language);

    //         if active.is_none() && !inactive {
    //             active = Some(i + 1);
    //         }

    //         let variant = Variant {
    //             name: all_variants[i].0.to_string(),
    //             code,
    //         };

    //         variants.push(variant);
    //     }

    //     let code = &variation_content
    //         [(all_variants[all_variants.len() - 1].2 + language.variant_header_end().len())..];

    //     let (code, inactive) = Code::remove_comments(code, language);

    //     if active.is_none() && !inactive {
    //         active = Some(all_variants.len());
    //     }

    //     let variant = Variant {
    //         name: all_variants[all_variants.len() - 1].0.to_string(),
    //         code,
    //     };

    //     variants.push(variant);

    //     if let Some(active) = active {
    //         Variation {
    //             name: None,
    //             base,
    //             variants,
    //             active,
    //         }
    //     } else {
    //         panic!("At least one variant must be active");
    //     }
    // }

    // fn get_variation<'a>(
    //     content: &'a str,
    //     language: &Language,
    // ) -> Option<(&'a str, Variation, &'a str)> {
    //     // get the next variation if it exists
    //     let begin = content.find(&language.variation_begin())? + language.variation_begin().len();
    //     let end = begin + content[begin..].find(&language.variation_end())?;

    //     let variation_content = &content[begin..end];
    //     let pref = &content[..begin - language.variation_begin().len()].trim();
    //     let rest = &content[end + language.variation_end().len()..].trim();

    //     let variation = Code::parse_variation(variation_content, language);

    //     Some((pref, variation, rest))
    // }

    // pub(crate) fn parse_code(path: String, content: &str, language: Language) -> Code {
    //     // parse the content and return a list of variations and constants
    //     let mut parts = Vec::new();
    //     let mut content = content.trim();

    //     // todo: currently, there's a bug with the tagged variations, they don't get parsed correctly

    //     // todo: save the whitespaces before variants, so that they can be restored

    //     let mut current_line = 1;

    //     while let Some((pref, variation, rest)) = Code::get_variation(content, &language) {
    //         parts.push(Span::constant(pref.to_string(), current_line));
    //         let pref_lines = pref.lines().count();
    //         let variation_lines = variation.lines();
    //         parts.push(Span::variation(variation, current_line + pref_lines));
    //         current_line += pref_lines + variation_lines + 1;
    //         content = rest;
    //     }

    //     parts.push(Span::constant(content.to_string(), current_line));

    //     Code {
    //         language,
    //         parts,
    //         path,
    //     }
    // }

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

    pub(crate) fn get_variations(&self) -> Vec<Variation> {
        self.parts
            .iter()
            .filter_map(|part| match &part.content {
                SpanContent::Variation(v) => Some(v.clone()),
                _ => None,
            })
            .collect()
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

        self.into_file(&self.path)
    }
}
