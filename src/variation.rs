use std::fmt::Display;

use crate::languages::Language;

/// A code is divided into variations and constants.
/// A variation is a part of a code that can be changed with several variants.
/// For each variation, some variant is currently active.

/// A variant is a part of a variation that can be used to replace the base code.
/// A variant has a name and a code.
/// The name is used to identify the variant.

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String, // name of the variant
    pub code: String, // code of the variant
}

#[derive(Debug, Clone)]
pub struct Variation {
    pub base: String,           // base code
    pub variants: Vec<Variant>, // list of variants
    pub active: usize,          // index of the active variant
}

impl Variation {
    pub fn get_active(&self) -> Variant {
        if self.active == 0 {
            return Variant {
                name: "base".to_string(),
                code: self.base.clone(),
            };
        } else {
            return self.variants[self.active - 1].clone();
        }
    }
}

type Constant = String;

#[derive(Debug, Clone)]
pub enum CodePart {
    Variation(Variation),
    Constant(Constant),
}

#[derive(Debug)]
pub struct Code {
    pub language: Language,
    pub parts: Vec<CodePart>,
}

impl Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut content = String::new();
        for part in &self.parts {
            match part {
                CodePart::Constant(c) => content.push_str(c),
                CodePart::Variation(v) => {
                    content.push_str("\n");
                    content.push_str(&self.language.variation_begin());
                    content.push_str("\n");
                    if v.active == 0 {
                        content.push_str(&v.base);
                    } else {
                        content.push_str(&self.language.variant_body_begin());
                        content.push_str("\n");
                        content.push_str(&v.base);
                        content.push_str("\n");
                        content.push_str(&self.language.variant_body_end());
                    }
                    content.push_str("\n");

                    for (i, variant) in v.variants.iter().enumerate() {
                        content.push_str(&self.language.variant_header_begin());
                        content.push_str(&variant.name);
                        content.push_str(&self.language.variant_header_end());
                        content.push_str("\n");
                        if i + 1 != v.active {
                            content.push_str(&self.language.variant_body_begin());
                            content.push_str("\n");
                        }
                        content.push_str(&variant.code);
                        content.push_str("\n");
                        if i + 1 != v.active {
                            content.push_str(&self.language.variant_body_end());
                            content.push_str("\n");
                        }
                    }
                    content.push_str(&self.language.variation_end());
                    content.push_str("\n");
                }
            }
        }
        write!(f, "{}", content)
    }
}

impl Code {
    pub fn from_file(filepath: &str) -> Result<Code, std::io::Error> {
        // read the file and parse it
        let file_content = std::fs::read_to_string(filepath)?;
        let language = Code::detect_language(filepath);
        let code = Code::parse_code(&file_content, language);
        Ok(code)
    }

    pub fn detect_language(filepath: &str) -> Language {
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
        let _ = loop {
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
                + &language.variant_header_begin().len())
                ..(header_end - &language.variant_header_end().len() + 1)]
                .split_whitespace()
                .next();

            if variant_name.is_none() {
                break;
            }

            headers.push((variant_name.unwrap().to_string(), header_begin, header_end));
            start = header_end;
        };

        headers
    }

    fn remove_comments(variant_content: &str, language: &Language) -> (String, bool) {
        let variant_content = variant_content.trim();
        let is_commented = variant_content.starts_with(&language.comment_begin())
            && variant_content.ends_with(&language.comment_end());
        if is_commented {
            let variant_content = variant_content.trim_start_matches(&language.variant_body_begin());
            let variant_content = variant_content.trim_end_matches(&language.variant_body_end());
            (variant_content.trim().to_string(), true)
        } else {
            (variant_content.trim().to_string(), false)
        }
    }

    fn parse_variation(variation_content: &str, language: &Language) -> Variation {
        // parse the variation content and return a variation
        let variation_content = variation_content.trim();
        let all_variants = Code::get_all_variant_headers(variation_content, language);

        if all_variants.is_empty() {
            panic!("Variation must have at least a variant");
        }

        let base = variation_content[..all_variants[0].1].to_string();
        let (base, is_base_inactive) = Code::remove_comments(&base, language);
        let mut variants = Vec::new();
        let mut active = if is_base_inactive { None } else { Some(0) };

        for i in 0..all_variants.len() - 1 {
            let code = &variation_content
                [(all_variants[i].2 + language.variant_header_end().len())..all_variants[i + 1].1];

            let (code, inactive) = Code::remove_comments(code, language);

            if active.is_none() && !inactive {
                active = Some(i + 1);
            }

            let variant = Variant {
                name: all_variants[i].0.to_string(),
                code,
            };

            variants.push(variant);
        }

        let code = &variation_content
            [(all_variants[all_variants.len() - 1].2 + language.variant_header_end().len())..];

        let (code, inactive) = Code::remove_comments(code, language);

        if active.is_none() && !inactive {
            active = Some(all_variants.len());
        }

        let variant = Variant {
            name: all_variants[all_variants.len() - 1].0.to_string(),
            code,
        };

        variants.push(variant);

        if let Some(active) = active {
            Variation {
                base,
                variants,
                active,
            }
        } else {
            panic!("At least one variant must be active");
        }
    }

    fn get_variation<'a>(
        content: &'a str,
        language: &Language,
    ) -> Option<(&'a str, Variation, &'a str)> {
        // get the next variation if it exists
        let begin = content.find(&language.variation_begin())? + language.variation_begin().len();
        let end = begin + content[begin..].find(&language.variation_end())?;

        let variation_content = &content[begin..end];
        let pref = &content[..begin - language.variation_begin().len()].trim();
        let rest = &content[end + language.variation_end().len()..].trim();

        let variation = Code::parse_variation(variation_content, language);

        Some((pref, variation, rest))
    }

    pub fn parse_code(content: &str, language: Language) -> Code {
        // parse the content and return a list of variations and constants
        let mut parts = Vec::new();
        let mut content = content.trim();

        while let Some((pref, variation, rest)) = Code::get_variation(content, &language) {
            parts.push(CodePart::Constant(pref.to_string()));
            parts.push(CodePart::Variation(variation));
            content = rest.trim();
        }

        parts.push(CodePart::Constant(content.to_string()));

        Code { language, parts }
    }
}

impl Code {
    fn longest_common_prefix(strs: &[String]) -> String {
        // Credits for this function goes to 
        // https://users.rust-lang.org/t/is-this-code-idiomatic/51798/14
        for (idx,c) in strs[0].char_indices() {
            // Because `s[..idx]` represents a common prefix,
            // `idx` must be a valid character boundary in all the strings
            if !strs[1..].iter().all(|s| s[idx..].chars().next() == Some(c)) {
                return strs[0][..idx].to_string();
            }
        }
        strs[0].to_string()
    }

    pub fn get_variations(&self) -> Vec<(String, Vec<String>, usize)> {
        let mut variations = Vec::new();
        for part in &self.parts {
            match part {
                CodePart::Variation(v) => {
                    let mut variant_names = Vec::new();
                    for variant in &v.variants {
                        variant_names.push(variant.name.clone());
                    }
                    let prefix = Code::longest_common_prefix(variant_names.as_slice());
                    variations.push((prefix, variant_names, v.active));
                }
                _ => {}
            }
        }
        variations
    }
}