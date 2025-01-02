#[derive(Debug)]
pub enum Language {
    Coq,
    Haskell,
    Racket,
    Rust,
}

impl Language {
    pub fn file_extension(&self) -> &str {
        match self {
            Language::Coq => "v",
            Language::Haskell => "hs",
            Language::Racket => "rkt",
            Language::Rust => "rs",
        }
    }

    pub fn extension_to_language(ext: &str) -> Option<Language> {
        match ext {
            "v" => Some(Language::Coq),
            "hs" => Some(Language::Haskell),
            "rkt" => Some(Language::Racket),
            "rs" => Some(Language::Rust),
            _ => None,
        }
    }

    pub fn comment_begin(&self) -> String {
        match self {
            Language::Coq => "(*".to_string(),
            Language::Haskell => "-{".to_string(),
            Language::Racket => "|#".to_string(),
            Language::Rust => "/*".to_string(),
        }
    }

    pub fn comment_end(&self) -> String {
        match self {
            Language::Coq => "*)".to_string(),
            Language::Haskell => "}-".to_string(),
            Language::Racket => "#|".to_string(),
            Language::Rust => "*/".to_string(),
        }
    }

    pub fn variation_begin(&self, name: &str) -> String {
        format!(r"{}! {}{}", self.comment_begin(), name, self.comment_end())
    }

    pub fn variation_end(&self) -> String {
        format!("{} !{}", self.comment_begin(), self.comment_end())
    }

    pub fn variant_header_begin(&self) -> String {
        format!("{}!!", self.comment_begin())
    }

    pub fn variant_header_end(&self) -> String {
        self.comment_end()
    }

    pub fn variant_body_begin(&self) -> String {
        format!("{}!", self.comment_begin())
    }

    pub fn variant_body_end(&self) -> String {
        self.comment_end()
    }
}
