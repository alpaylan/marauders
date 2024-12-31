/// A code is divided into variations and constants.
/// A variation is a part of a code that can be changed with several variants.
/// For each variation, some variant is currently active.

use std::fmt::Display;

/// A variant is a part of a variation that can be used to replace the base code.
/// A variant has a name and a code.
/// The name is used to identify the variant.
#[derive(Debug, Clone)]
pub struct Variant {
    /// name of the variant
    pub name: String,
    /// code of the variant
    pub code: String,
}

#[derive(Debug, Clone)]
pub struct Variation {
    /// name of the variation
    pub name: Option<String>,   
    /// base code
    pub base: String,           
    /// list of variants
    pub variants: Vec<Variant>, 
    /// index of the active variant
    pub active: usize,          
}

impl Display for Variation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {

        let name = if self.name.is_some() {
            self.name.as_ref().unwrap()
        } else {
            "anonymous"
        };
        
        let active = if self.active == 0 {
            "base"
        } else {
            &self.variants[self.active - 1].name
        };

        let content = format!(
            "(name: {}, active: {}, variants: {:?})",
            name, active, self.variants.iter().map(|v| v.name.to_string()).collect::<Vec<String>>()
        );

        write!(f, "{}", content)
    }
}
