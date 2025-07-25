use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum Language {
    Rocq,
    Haskell,
    Racket,
    Rust,
    OCaml,
    Python,
    #[serde(untagged)]
    Custom(CustomLanguage),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CustomLanguage {
    pub name: String,
    pub extension: String,
    pub comment_begin: String,
    pub comment_end: String,
    pub mutation_marker: String,
}

impl Language {
    pub fn file_extension(&self) -> &str {
        match self {
            Language::Rocq => "v",
            Language::Haskell => "hs",
            Language::Racket => "rkt",
            Language::Rust => "rs",
            Language::OCaml => "ml",
            Language::Python => "py",
            Language::Custom(custom) => custom.extension.as_str(),
        }
    }

    pub fn extension_to_language(
        ext: &str,
        custom_languages: &Vec<CustomLanguage>,
    ) -> Option<Language> {
        match ext {
            "v" => Some(Language::Rocq),
            "hs" => Some(Language::Haskell),
            "rkt" => Some(Language::Racket),
            "rs" => Some(Language::Rust),
            "ml" => Some(Language::OCaml),
            _ => {
                for custom in custom_languages {
                    if custom.extension == ext {
                        return Some(Language::Custom(custom.clone()));
                    }
                }
                None
            }
        }
    }

    pub fn name_to_language(
        name: &str,
        custom_languages: &Vec<CustomLanguage>,
    ) -> Option<Language> {
        match name.to_lowercase().as_str() {
            "rocq" => Some(Language::Rocq),
            "haskell" => Some(Language::Haskell),
            "racket" => Some(Language::Racket),
            "rust" => Some(Language::Rust),
            "ocaml" => Some(Language::OCaml),
            "python" => Some(Language::Python),
            _ => {
                for custom in custom_languages {
                    if custom.name == name {
                        return Some(Language::Custom(custom.clone()));
                    }
                }
                log::warn!("unknown language: {}", name);
                None
            }
        }
    }

    pub fn comment_begin(&self) -> String {
        match self {
            Language::Rocq => "(*".to_string(),
            Language::Haskell => "{-".to_string(),
            Language::Racket => "#|".to_string(),
            Language::Rust => "/*".to_string(),
            Language::OCaml => "(*".to_string(),
            Language::Python => "#".to_string(),
            Language::Custom(custom) => custom.comment_begin.clone(),
        }
    }

    pub fn comment_end(&self) -> String {
        match self {
            Language::Rocq => "*)".to_string(),
            Language::Haskell => "-}".to_string(),
            Language::Racket => "|#".to_string(),
            Language::Rust => "*/".to_string(),
            Language::OCaml => "*)".to_string(),
            Language::Custom(custom) => custom.comment_end.clone(),
            Language::Python => "\n".to_string(),
        }
    }

    pub fn variation_begin(&self, name: &str) -> String {
        format!(
            r"{}{} {}{}",
            self.comment_begin(),
            self.mutation_marker(),
            name,
            self.comment_end()
        )
    }

    pub fn variation_end(&self) -> String {
        format!(
            "{} {}{}",
            self.comment_begin(),
            self.mutation_marker(),
            self.comment_end()
        )
    }

    pub fn variant_header_begin(&self) -> String {
        format!(
            "{}{}{}",
            self.comment_begin(),
            self.mutation_marker(),
            self.mutation_marker()
        )
    }

    pub fn variant_header_end(&self) -> String {
        self.comment_end()
    }

    pub fn variant_body_begin(&self) -> String {
        format!("{}{}", self.comment_begin(), self.mutation_marker())
    }

    pub fn variant_body_end(&self) -> String {
        self.comment_end()
    }

    pub fn mutation_marker(&self) -> &str {
        match self {
            Language::Rocq
            | Language::Haskell
            | Language::Racket
            | Language::OCaml
            | Language::Python => "!",
            Language::Rust => "|",
            Language::Custom(custom_language) => custom_language.mutation_marker.as_str(),
        }
    }
}
