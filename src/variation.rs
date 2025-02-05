/// A code is divided into variations and constants.
/// A variation is a part of a code that can be changed with several variants.
/// For each variation, some variant is currently active.
use std::fmt::Display;

/// A variant is a part of a variation that can be used to replace the base code.
/// A variant has a name and a code.
/// The name is used to identify the variant.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    /// name of the variant
    pub name: String,
    /// code of the variant(as lines)
    pub body: VariantBody,
}

impl Variant {
    pub(crate) fn lines(&self) -> Vec<String> {
        self.body.lines()
    }

    pub(crate) fn indentation(&self) -> Option<String> {
        self.body.indentation()
    }

    pub(crate) fn is_active(&self) -> bool {
        self.body.is_active()
    }

    pub(crate) fn activate(&mut self) {
        match &mut self.body {
            VariantBody::InactiveMultiLine { lines, .. } => {
                self.body = VariantBody::Active {
                    lines: lines.clone(),
                };
            }
            VariantBody::InactiveSingleLine { line, indentation } => {
                let line = format!("{}{}", indentation, line);
                self.body = VariantBody::Active {
                    lines: vec![line.clone()],
                };
            }
            VariantBody::Active { .. } => {}
        }
    }

    pub(crate) fn deactivate(&mut self) {
        match &mut self.body {
            VariantBody::InactiveMultiLine { .. } => {}
            VariantBody::InactiveSingleLine { line, .. } => {
                self.body = VariantBody::InactiveSingleLine {
                    line: line.clone(),
                    indentation: "".to_string(),
                };
            }
            VariantBody::Active { lines } => {
                let line = lines.join("\n");
                self.body = VariantBody::InactiveMultiLine {
                    lines: vec![line],
                    indentation: "".to_string(),
                };
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariantBody {
    InactiveMultiLine {
        lines: Vec<String>,
        indentation: String,
    },
    InactiveSingleLine {
        line: String,
        indentation: String,
    },
    Active {
        lines: Vec<String>,
    },
}

impl VariantBody {
    pub(crate) fn lines(&self) -> Vec<String> {
        match self {
            VariantBody::InactiveMultiLine { lines, .. } => lines.clone(),
            VariantBody::InactiveSingleLine { line, .. } => vec![line.clone()],
            VariantBody::Active { lines } => lines.clone(),
        }
    }

    pub(crate) fn count_lines(&self) -> usize {
        match self {
            VariantBody::InactiveMultiLine { lines, .. } | VariantBody::Active { lines } => {
                lines.len()
            }
            VariantBody::InactiveSingleLine { .. } => 1,
        }
    }

    pub(crate) fn indentation(&self) -> Option<String> {
        match self {
            VariantBody::InactiveMultiLine { indentation, .. } => Some(indentation.clone()),
            VariantBody::InactiveSingleLine { indentation, .. } => Some(indentation.clone()),
            VariantBody::Active { .. } => None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        match self {
            VariantBody::Active { .. } => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variation {
    /// name of the variation
    pub name: Option<String>,
    /// tags of the variation
    pub tags: Vec<String>,
    /// base code
    pub base: Variant,
    /// list of variants
    pub variants: Vec<Variant>,
    /// index of the active variant
    pub active: usize,
    /// indentation of the variation
    pub indentation: String,
}

impl Variation {
    pub(crate) fn activate_base(&mut self) {
        self.active = 0;
        self.base.activate();
        for variant in self.variants.iter_mut() {
            variant.deactivate();
        }
    }

    pub(crate) fn activate_variant(&mut self, index: usize) {
        if index == 0 {
            self.activate_base();
            return;
        }

        self.active = index;
        let index = index - 1;
        self.base.deactivate();
        for (i, variant) in self.variants.iter_mut().enumerate() {
            if i == index {
                variant.activate();
            } else {
                variant.deactivate();
            }
        }
    }
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
            "(name: {}, active: {}, variants: {:?}, tags: {:?})",
            name,
            active,
            self.variants
                .iter()
                .map(|v| v.name.to_string())
                .collect::<Vec<String>>(),
            self.tags
        );

        write!(f, "{}", content)
    }
}
