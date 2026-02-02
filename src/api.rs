//! Library API for programmatic access to Marauders functionality.
//!
//! This module provides structured data-returning functions for use by
//! GUIs, web services, tests, and other programmatic consumers.
//!
//! Unlike the CLI commands in `commands.rs`, these functions return
//! structured results instead of printing to stdout.

use std::path::{Path, PathBuf};

use crate::{Language, Project, ProjectConfig, SpanContent};

/// Information about a variation in the project.
#[derive(Debug, Clone, PartialEq)]
pub struct VariationInfo {
    /// Path to the file containing this variation
    pub path: PathBuf,
    /// Line number where the variation starts (1-indexed)
    pub line: usize,
    /// Optional name of the variation
    pub name: Option<String>,
    /// Names of all variants in this variation
    pub variants: Vec<String>,
    /// Index of the currently active variant (0 = base)
    pub active: usize,
    /// Tags associated with this variation
    pub tags: Vec<String>,
}

/// Result of a set/unset operation.
#[derive(Debug, Clone, PartialEq)]
pub struct SetResult {
    /// Path to the file that was modified
    pub file: PathBuf,
    /// Name of the variation that was modified (if named)
    pub variation: Option<String>,
    /// Previous active variant index (0 = base)
    pub previous_active: usize,
    /// New active variant index (0 = base)
    pub new_active: usize,
}

/// Error type for API operations.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// The specified variant was not found
    VariantNotFound {
        variant: String,
        available: Vec<String>,
    },
    /// The variant is already active
    AlreadyActive { variant: String },
    /// Project error
    ProjectError(String),
    /// IO error
    IoError(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::VariantNotFound { variant, available } => {
                write!(
                    f,
                    "variant '{}' not found, available variants: {:?}",
                    variant, available
                )
            }
            ApiError::AlreadyActive { variant } => {
                write!(f, "variant '{}' is already active", variant)
            }
            ApiError::ProjectError(msg) => write!(f, "project error: {}", msg),
            ApiError::IoError(msg) => write!(f, "IO error: {}", msg),
        }
    }
}

impl std::error::Error for ApiError {}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError::ProjectError(err.to_string())
    }
}

impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        ApiError::IoError(err.to_string())
    }
}

/// Returns all variations in a project with their locations and metadata.
///
/// # Arguments
///
/// * `project` - The project to list variations from
///
/// # Returns
///
/// A vector of `VariationInfo` structs describing each variation found.
pub fn list_variations(project: &Project) -> Vec<VariationInfo> {
    let mut result = Vec::new();

    for file in &project.files {
        for span in &file.code.spans {
            if let SpanContent::Variation(v) = &span.content {
                result.push(VariationInfo {
                    path: file.path.clone(),
                    line: span.line,
                    name: v.name.clone(),
                    variants: v.variants.iter().map(|var| var.name.clone()).collect(),
                    active: v.active,
                    tags: v.tags.clone(),
                });
            }
        }
    }

    result
}

/// Sets a variant as active, returning what was changed.
///
/// # Arguments
///
/// * `project` - The project to modify (mutably)
/// * `variant` - The name of the variant to activate
///
/// # Returns
///
/// * `Ok(SetResult)` - Information about what was changed
/// * `Err(ApiError)` - If the variant was not found or is already active
pub fn set_variant(project: &mut Project, variant: &str) -> Result<SetResult, ApiError> {
    let mut all_variants = Vec::new();

    for file in project.files.iter_mut() {
        let code = &mut file.code;

        // Find the variation containing this variant
        if let Some((variation_index, span)) =
            code.spans
                .iter()
                .enumerate()
                .find(|(_, s)| match &s.content {
                    SpanContent::Variation(v) => v.variants.iter().any(|var| var.name == variant),
                    _ => false,
                })
        {
            let variation = match &span.content {
                SpanContent::Variation(v) => v,
                _ => unreachable!(),
            };

            let previous_active = variation.active;
            let variation_name = variation.name.clone();

            // Find the variant index
            let (variant_index, _) = variation
                .variants
                .iter()
                .enumerate()
                .find(|(_, v)| v.name == variant)
                .ok_or_else(|| ApiError::VariantNotFound {
                    variant: variant.to_string(),
                    available: variation.variants.iter().map(|v| v.name.clone()).collect(),
                })?;

            // Shift index by 1 because 0 is reserved for base
            let new_active = variant_index + 1;

            if previous_active == new_active {
                return Err(ApiError::AlreadyActive {
                    variant: variant.to_string(),
                });
            }

            code.set_active_variant(variation_index, new_active)
                .map_err(|e| ApiError::ProjectError(e.to_string()))?;

            return Ok(SetResult {
                file: file.path.clone(),
                variation: variation_name,
                previous_active,
                new_active,
            });
        } else {
            // Collect variants from this file for error reporting
            all_variants.extend(code.get_all_variants());
        }
    }

    Err(ApiError::VariantNotFound {
        variant: variant.to_string(),
        available: all_variants,
    })
}

/// Unsets a variant (resets its variation to base), returning what was changed.
///
/// # Arguments
///
/// * `project` - The project to modify (mutably)
/// * `variant` - The name of the variant whose variation should be reset to base
///
/// # Returns
///
/// * `Ok(SetResult)` - Information about what was changed
/// * `Err(ApiError)` - If the variant was not found
pub fn unset_variant(project: &mut Project, variant: &str) -> Result<SetResult, ApiError> {
    let mut all_variants = Vec::new();

    for file in project.files.iter_mut() {
        let code = &mut file.code;

        // Find the variation containing this variant
        if let Some((variation_index, span)) =
            code.spans
                .iter()
                .enumerate()
                .find(|(_, s)| match &s.content {
                    SpanContent::Variation(v) => v.variants.iter().any(|var| var.name == variant),
                    _ => false,
                })
        {
            let variation = match &span.content {
                SpanContent::Variation(v) => v,
                _ => unreachable!(),
            };

            let previous_active = variation.active;
            let variation_name = variation.name.clone();

            // Reset to base (index 0)
            code.set_active_variant(variation_index, 0)
                .map_err(|e| ApiError::ProjectError(e.to_string()))?;

            return Ok(SetResult {
                file: file.path.clone(),
                variation: variation_name,
                previous_active,
                new_active: 0,
            });
        } else {
            all_variants.extend(code.get_all_variants());
        }
    }

    Err(ApiError::VariantNotFound {
        variant: variant.to_string(),
        available: all_variants,
    })
}

/// Resets all variations to base in the project.
///
/// # Arguments
///
/// * `project` - The project to reset (mutably)
///
/// # Returns
///
/// * `Ok(Vec<SetResult>)` - Information about all variations that were reset
/// * `Err(ApiError)` - If an error occurred during reset
pub fn reset_all(project: &mut Project) -> Result<Vec<SetResult>, ApiError> {
    let mut results = Vec::new();

    for file in project.files.iter_mut() {
        for span in file.code.spans.iter_mut() {
            if let SpanContent::Variation(v) = &mut span.content {
                if v.active != 0 {
                    let previous_active = v.active;
                    let variation_name = v.name.clone();

                    v.active = 0;
                    v.activate_base();

                    results.push(SetResult {
                        file: file.path.clone(),
                        variation: variation_name,
                        previous_active,
                        new_active: 0,
                    });
                }
            }
        }

        file.code
            .save_to_file(&file.path)
            .map_err(|e| ApiError::ProjectError(e.to_string()))?;
    }

    Ok(results)
}

/// Initializes a project configuration file.
///
/// # Arguments
///
/// * `path` - The directory where the config file should be created
/// * `language` - The primary language for the project (e.g., "rust", "python")
/// * `use_gitignore` - Whether to respect .gitignore when scanning files
///
/// # Returns
///
/// * `Ok(PathBuf)` - The path to the created config file
/// * `Err(ApiError)` - If the config file could not be created
pub fn init_project(path: &Path, language: &str, use_gitignore: bool) -> Result<PathBuf, ApiError> {
    let project_config = ProjectConfig {
        languages: Language::name_to_language(language, &vec![]).map_or(vec![], |l| vec![l]),
        custom_languages: vec![],
        ignore: vec![],
        use_gitignore,
    };

    let config_path = path.join("marauder.toml");
    let config_content =
        toml::to_string(&project_config).map_err(|e| ApiError::ProjectError(e.to_string()))?;

    std::fs::write(&config_path, config_content)?;

    Ok(config_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_variations() {
        let project = Project::new(Path::new("test"), None).unwrap();
        let variations = list_variations(&project);

        // Should find variations in test files
        assert!(!variations.is_empty());

        // Check that each variation has the expected structure
        for info in &variations {
            assert!(info.path.exists() || info.path.starts_with("test/"));
            assert!(info.line > 0);
        }
    }

    #[test]
    fn test_variation_info_structure() {
        let info = VariationInfo {
            path: PathBuf::from("test.rs"),
            line: 10,
            name: Some("my_variation".to_string()),
            variants: vec!["variant_a".to_string(), "variant_b".to_string()],
            active: 0,
            tags: vec!["tag1".to_string()],
        };

        assert_eq!(info.path, PathBuf::from("test.rs"));
        assert_eq!(info.line, 10);
        assert_eq!(info.name, Some("my_variation".to_string()));
        assert_eq!(info.variants.len(), 2);
        assert_eq!(info.active, 0);
        assert_eq!(info.tags.len(), 1);
    }

    #[test]
    fn test_set_result_structure() {
        let result = SetResult {
            file: PathBuf::from("test.rs"),
            variation: Some("my_variation".to_string()),
            previous_active: 0,
            new_active: 1,
        };

        assert_eq!(result.file, PathBuf::from("test.rs"));
        assert_eq!(result.variation, Some("my_variation".to_string()));
        assert_eq!(result.previous_active, 0);
        assert_eq!(result.new_active, 1);
    }

    #[test]
    fn test_api_error_display() {
        let err = ApiError::VariantNotFound {
            variant: "foo".to_string(),
            available: vec!["bar".to_string(), "baz".to_string()],
        };
        let msg = format!("{}", err);
        assert!(msg.contains("foo"));
        assert!(msg.contains("bar"));

        let err = ApiError::AlreadyActive {
            variant: "foo".to_string(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("already active"));
    }
}
