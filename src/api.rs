//! Library API for programmatic access to Marauders functionality.
//!
//! This module provides structured data-returning functions for use by
//! GUIs, web services, tests, and other programmatic consumers.
//!
//! Unlike the CLI commands in `commands.rs`, these functions return
//! structured results instead of printing to stdout.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Target format for syntax conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionTarget {
    /// Convert comment syntax to Rust functional syntax.
    RustFunctional,
    /// Convert Rust functional syntax to comment syntax.
    RustComment,
    /// Convert comment syntax to preprocessor syntax.
    Preprocessor,
    /// Convert comment syntax to patch syntax.
    Patch,
    /// Convert comment syntax to match-replace syntax.
    MatchReplace,
    /// Convert supported syntaxes to comment syntax.
    ///
    /// Supported source syntaxes:
    /// - Rust functional syntax (`.rs`)
    /// - Preprocessor syntax (`#if defined(M_...)`)
    /// - Patch syntax (`format = "marauders_patch_bundle"` manifest)
    /// - Match-replace syntax (JSON object/array with `scope`, `match`, and `variants`)
    Comment,
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
        if let Some((variation_index, span)) = code.spans.iter().enumerate().find(|(_, s)| match &s
            .content
        {
            SpanContent::Variation(v) => v.variants.iter().any(|var| var.name == variant),
            _ => false,
        }) {
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
        if let Some((variation_index, span)) = code.spans.iter().enumerate().find(|(_, s)| match &s
            .content
        {
            SpanContent::Variation(v) => v.variants.iter().any(|var| var.name == variant),
            _ => false,
        }) {
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

/// Converts a file's mutation syntax in place.
///
/// Currently supported:
/// - Rust comment syntax -> Rust functional syntax.
/// - Rust functional syntax -> comment syntax.
/// - Comment syntax -> preprocessor syntax.
/// - Comment syntax -> patch syntax.
/// - Comment syntax -> match-replace syntax.
/// - Preprocessor syntax -> comment syntax.
/// - Patch syntax -> comment syntax.
/// - Match-replace syntax -> comment syntax.
pub fn convert_file(path: &Path, target: ConversionTarget) -> Result<PathBuf, ApiError> {
    if !path.is_file() {
        return Err(ApiError::ProjectError(format!(
            "path '{}' is not a file",
            path.display()
        )));
    }

    match target {
        ConversionTarget::RustFunctional => {
            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .ok_or_else(|| {
                    ApiError::ProjectError("file extension is not valid unicode".to_string())
                })?;

            if extension != "rs" {
                return Err(ApiError::ProjectError(format!(
                    "Rust functional conversion is only supported for .rs files (got '.{}')",
                    extension
                )));
            }

            let content = std::fs::read_to_string(path)?;
            let language = crate::syntax::functional::functional_language_for_extension(extension)
                .ok_or_else(|| {
                    ApiError::ProjectError(format!(
                        "no functional mutation backend is available for '.{}'",
                        extension
                    ))
                })?;
            let spans = crate::syntax::comment::parse_code(&content)
                .map_err(|e| ApiError::ProjectError(e.to_string()))?;
            let converted =
                crate::syntax::functional::render_functional_code(language, &content, &spans)
                    .map_err(|e| ApiError::ProjectError(e.to_string()))?;
            std::fs::write(path, converted)?;
        }
        ConversionTarget::RustComment => {
            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .ok_or_else(|| {
                    ApiError::ProjectError("file extension is not valid unicode".to_string())
                })?;

            if extension != "rs" {
                return Err(ApiError::ProjectError(format!(
                    "Rust comment conversion is only supported for .rs files (got '.{}')",
                    extension
                )));
            }

            let content = std::fs::read_to_string(path)?;
            let language = crate::syntax::functional::functional_language_for_extension(extension)
                .ok_or_else(|| {
                    ApiError::ProjectError(format!(
                        "no functional mutation backend is available for '.{}'",
                        extension
                    ))
                })?;
            let converted =
                crate::syntax::functional::render_comment_code_from_functional(language, &content)
                    .map_err(|e| ApiError::ProjectError(e.to_string()))?;
            std::fs::write(path, converted)?;
        }
        ConversionTarget::Preprocessor => {
            let content = std::fs::read_to_string(path)?;
            let spans = crate::syntax::comment::parse_code(&content)
                .map_err(|e| ApiError::ProjectError(e.to_string()))?;
            let converted =
                crate::syntax::preprocessor::render_preprocessor_code_from_comment(&spans)
                    .map_err(|e| ApiError::ProjectError(e.to_string()))?;
            std::fs::write(path, converted)?;
        }
        ConversionTarget::Patch => {
            let content = std::fs::read_to_string(path)?;
            let spans = crate::syntax::comment::parse_code(&content)
                .map_err(|e| ApiError::ProjectError(e.to_string()))?;
            let rendered = crate::syntax::patch::render_patch_bundle_from_comment(
                &spans,
                &path.to_string_lossy(),
            )
            .map_err(|e| ApiError::ProjectError(e.to_string()))?;
            std::fs::write(path, &rendered.base_source)?;
            let bundle_dir = crate::syntax::patch::patch_bundle_dir_for_source(path)
                .map_err(|e| ApiError::ProjectError(e.to_string()))?;
            let manifest_path = crate::syntax::patch::write_patch_bundle(&bundle_dir, &rendered)
                .map_err(|e| ApiError::ProjectError(e.to_string()))?;
            return Ok(manifest_path);
        }
        ConversionTarget::MatchReplace => {
            let content = std::fs::read_to_string(path)?;
            let spans = crate::syntax::comment::parse_code(&content)
                .map_err(|e| ApiError::ProjectError(e.to_string()))?;
            let converted = crate::syntax::match_replace::render_match_replace_code_from_comment(
                &spans,
                &path.to_string_lossy(),
            )
            .map_err(|e| ApiError::ProjectError(e.to_string()))?;

            // Keep the original source file as the base program (with mutations stripped).
            let mut base_source = String::new();
            for span in &spans {
                match &span.content {
                    SpanContent::Line(line) => base_source.push_str(line),
                    SpanContent::Variation(variation) => {
                        for line in variation.base.lines() {
                            base_source.push_str(&line);
                            base_source.push('\n');
                        }
                    }
                }
            }
            std::fs::write(path, base_source)?;

            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| ApiError::ProjectError("invalid source filename".to_string()))?;
            let out_path = path.with_file_name(format!("{file_name}.match_replace.json"));
            std::fs::write(&out_path, converted)?;
            return Ok(out_path);
        }
        ConversionTarget::Comment => {
            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .ok_or_else(|| {
                    ApiError::ProjectError("file extension is not valid unicode".to_string())
                })?;
            let content = std::fs::read_to_string(path)?;

            if extension == "rs" {
                if let Some(language) =
                    crate::syntax::functional::functional_language_for_extension(extension)
                {
                    if crate::syntax::functional::looks_like_mutations(language, &content) {
                        let converted =
                            crate::syntax::functional::render_comment_code_from_functional(
                                language, &content,
                            )
                            .map_err(|e| ApiError::ProjectError(e.to_string()))?;
                        std::fs::write(path, converted)?;
                        return Ok(path.to_path_buf());
                    }
                }
            }

            if crate::syntax::preprocessor::looks_like_mutations(&content) {
                let language =
                    Language::extension_to_language(extension, &vec![]).unwrap_or(Language::Rust);
                let converted = crate::syntax::preprocessor::render_comment_code_from_preprocessor(
                    language, &content,
                )
                .map_err(|e| ApiError::ProjectError(e.to_string()))?;
                std::fs::write(path, converted)?;
            } else if crate::syntax::patch::looks_like_mutations(&content) {
                let (source_path, converted) =
                    crate::syntax::patch::render_comment_code_from_patch(path, &content)
                        .map_err(|e| ApiError::ProjectError(e.to_string()))?;
                std::fs::write(&source_path, converted)?;
                return Ok(source_path);
            } else if crate::syntax::match_replace::looks_like_mutations(&content) {
                let (source_path, converted) =
                    crate::syntax::match_replace::render_comment_code_from_match_replace(&content)
                        .map_err(|e| ApiError::ProjectError(e.to_string()))?;
                std::fs::write(&source_path, converted)?;
                return Ok(source_path);
            } else {
                return Err(ApiError::ProjectError(
                    "unable to detect source syntax for conversion to comment".to_string(),
                ));
            }
        }
    }

    Ok(path.to_path_buf())
}

/// Imports externally generated Rust mutants into Marauders comment mutation syntax.
///
/// The `base_path` must point to the original `.rs` file.
/// Each path in `mutant_paths` must point to a mutated variant of the same file.
pub fn import_rust_mutants(
    base_path: &Path,
    mutant_paths: &[PathBuf],
    output_path: Option<&Path>,
    name_prefix: &str,
) -> Result<PathBuf, ApiError> {
    if !base_path.is_file() {
        return Err(ApiError::ProjectError(format!(
            "base path '{}' is not a file",
            base_path.display()
        )));
    }
    if base_path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return Err(ApiError::ProjectError(format!(
            "Rust mutant import expects a .rs base file (got '{}')",
            base_path.display()
        )));
    }
    if mutant_paths.is_empty() {
        return Err(ApiError::ProjectError(
            "expected at least one mutant path".to_string(),
        ));
    }

    let base_source = normalize_external_mutant_source(&std::fs::read_to_string(base_path)?);
    let mut mutant_sources = Vec::new();
    for mutant_path in mutant_paths {
        if !mutant_path.is_file() {
            return Err(ApiError::ProjectError(format!(
                "mutant path '{}' is not a file",
                mutant_path.display()
            )));
        }
        if mutant_path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            return Err(ApiError::ProjectError(format!(
                "Rust mutant import expects .rs mutants (got '{}')",
                mutant_path.display()
            )));
        }
        mutant_sources.push(normalize_external_mutant_source(&std::fs::read_to_string(
            mutant_path,
        )?));
    }

    let converted = crate::syntax::functional::import_rust_mutants_from_files(
        &base_source,
        &mutant_sources,
        name_prefix,
    )
    .map_err(|e| ApiError::ProjectError(e.to_string()))?;

    let destination = output_path.unwrap_or(base_path);
    std::fs::write(destination, converted)?;
    Ok(destination.to_path_buf())
}

/// Imports Rust mutants directly from a cargo-mutants output directory.
///
/// This reads unified diff files from `<cargo_mutants_dir>/diff` (or directly from
/// `cargo_mutants_dir` if no `diff` subdirectory exists), applies matching diffs to
/// `base_path`, and imports the resulting mutants into Marauders comment syntax.
pub fn import_rust_mutants_from_cargo_mutants_output(
    base_path: &Path,
    cargo_mutants_dir: &Path,
    output_path: Option<&Path>,
    name_prefix: &str,
) -> Result<PathBuf, ApiError> {
    if !base_path.is_file() {
        return Err(ApiError::ProjectError(format!(
            "base path '{}' is not a file",
            base_path.display()
        )));
    }
    if base_path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return Err(ApiError::ProjectError(format!(
            "Rust mutant import expects a .rs base file (got '{}')",
            base_path.display()
        )));
    }
    if !cargo_mutants_dir.is_dir() {
        return Err(ApiError::ProjectError(format!(
            "cargo-mutants dir '{}' is not a directory",
            cargo_mutants_dir.display()
        )));
    }

    let base_source = normalize_external_mutant_source(&std::fs::read_to_string(base_path)?);
    let mutant_sources = collect_mutant_sources_from_cargo_mutants_output(
        base_path,
        cargo_mutants_dir,
        &base_source,
    )?;
    if mutant_sources.is_empty() {
        return Err(ApiError::ProjectError(format!(
            "no applicable Rust mutants for '{}' were found in '{}'",
            base_path.display(),
            cargo_mutants_dir.display()
        )));
    }

    let converted = crate::syntax::functional::import_rust_mutants_from_files(
        &base_source,
        &mutant_sources,
        name_prefix,
    )
    .map_err(|e| ApiError::ProjectError(e.to_string()))?;

    let destination = output_path.unwrap_or(base_path);
    std::fs::write(destination, converted)?;
    Ok(destination.to_path_buf())
}

/// Runs `cargo mutants` for the project containing `base_path` and imports the generated
/// mutants into Marauders comment mutation syntax.
///
/// This is a convenience wrapper for fully automated external-mutant import:
/// 1) discover Cargo project root from `base_path`,
/// 2) run `cargo mutants --check --output <tmpdir>`,
/// 3) import diff output for `base_path` into `output_path`.
pub fn auto_generate_and_import_rust_mutants(
    base_path: &Path,
    output_path: &Path,
    name_prefix: &str,
    write_diffs_to_workdir: bool,
) -> Result<PathBuf, ApiError> {
    if !base_path.is_file() {
        return Err(ApiError::ProjectError(format!(
            "base path '{}' is not a file",
            base_path.display()
        )));
    }
    if base_path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return Err(ApiError::ProjectError(format!(
            "Rust mutant import expects a .rs base file (got '{}')",
            base_path.display()
        )));
    }

    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let synthetic_project_root =
        std::env::temp_dir().join(format!("marauders_single_file_project_{pid}_{nanos}"));
    let (project_root, project_file_rel, cleanup_synthetic_project) =
        if let Some(project_root) = find_cargo_project_root(base_path) {
            (
                project_root.clone(),
                base_path
                    .strip_prefix(&project_root)
                    .ok()
                    .map(PathBuf::from),
                false,
            )
        } else {
            let rel = create_single_file_cargo_project(base_path, &synthetic_project_root)?;
            (synthetic_project_root.clone(), Some(rel), true)
        };

    let cargo_mutants_out =
        std::env::temp_dir().join(format!("marauders_cargo_mutants_{pid}_{nanos}"));
    std::fs::create_dir_all(&cargo_mutants_out)?;

    let mut command = Command::new("cargo");
    command
        .arg("mutants")
        .arg("--check")
        .arg("--baseline")
        .arg("skip")
        .arg("--output")
        .arg(&cargo_mutants_out);
    if let Some(rel) = &project_file_rel {
        command.arg("--file").arg(rel);
    } else if let Ok(rel) = base_path.strip_prefix(&project_root) {
        command.arg("--file").arg(rel);
    }
    let run = command.current_dir(&project_root).output().map_err(|e| {
        ApiError::ProjectError(format!(
            "failed to run 'cargo mutants' in '{}': {}",
            project_root.display(),
            e
        ))
    })?;
    let copied_diffs = if write_diffs_to_workdir {
        Some(copy_cargo_mutants_diffs_to_workdir(&cargo_mutants_out))
    } else {
        None
    };

    let result = import_rust_mutants_from_cargo_mutants_output(
        base_path,
        &cargo_mutants_out,
        Some(output_path),
        name_prefix,
    );

    if let Some(copy_result) = copied_diffs {
        if let Err(copy_err) = copy_result {
            let _ = std::fs::remove_dir_all(&cargo_mutants_out);
            if cleanup_synthetic_project {
                let _ = std::fs::remove_dir_all(&project_root);
            }
            return Err(copy_err);
        }
    }

    if result.is_err() && !run.status.success() {
        let stderr = String::from_utf8_lossy(&run.stderr);
        let stdout = String::from_utf8_lossy(&run.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };
        let _ = std::fs::remove_dir_all(&cargo_mutants_out);
        if cleanup_synthetic_project {
            let _ = std::fs::remove_dir_all(&project_root);
        }
        return Err(ApiError::ProjectError(format!(
            "'cargo mutants' failed for '{}': {}",
            project_root.display(),
            detail
        )));
    }

    let _ = std::fs::remove_dir_all(&cargo_mutants_out);
    if cleanup_synthetic_project {
        let _ = std::fs::remove_dir_all(&project_root);
    }
    result
}

/// Collects Rust mutant file paths from a directory (e.g. `cargo-mutants` output).
///
/// Files are filtered to `.rs` files with the same file name as `base_path`,
/// parseable Rust source, and source content different from the base file.
pub fn collect_rust_mutants_from_dir(
    base_path: &Path,
    mutants_dir: &Path,
) -> Result<Vec<PathBuf>, ApiError> {
    if !base_path.is_file() {
        return Err(ApiError::ProjectError(format!(
            "base path '{}' is not a file",
            base_path.display()
        )));
    }
    if !mutants_dir.is_dir() {
        return Err(ApiError::ProjectError(format!(
            "mutants dir '{}' is not a directory",
            mutants_dir.display()
        )));
    }
    if base_path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return Err(ApiError::ProjectError(format!(
            "Rust mutant import expects a .rs base file (got '{}')",
            base_path.display()
        )));
    }

    let base_source = normalize_external_mutant_source(&std::fs::read_to_string(base_path)?);
    let base_name = base_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ApiError::ProjectError("base file name is not valid unicode".to_string()))?;

    let base_canonical = std::fs::canonicalize(base_path).ok();
    let base_rel_to_cwd = std::env::current_dir()
        .ok()
        .and_then(|cwd| base_path.strip_prefix(cwd).ok().map(|p| p.to_path_buf()));

    let mut preferred = Vec::<PathBuf>::new();
    let mut fallback = Vec::<PathBuf>::new();

    for entry in walk_files_recursive(mutants_dir)? {
        if entry.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        if entry.file_name().and_then(|name| name.to_str()) != Some(base_name) {
            continue;
        }
        if let Some(base_canonical) = &base_canonical {
            if let Ok(candidate_canonical) = std::fs::canonicalize(&entry) {
                if &candidate_canonical == base_canonical {
                    continue;
                }
            }
        }

        let source = match std::fs::read_to_string(&entry) {
            Ok(source) => normalize_external_mutant_source(&source),
            Err(_) => continue,
        };
        if source == base_source {
            continue;
        }
        if syn::parse_file(&source).is_err() {
            continue;
        }

        if let Some(base_rel) = &base_rel_to_cwd {
            if path_ends_with(&entry, base_rel) {
                preferred.push(entry);
            } else {
                fallback.push(entry);
            }
        } else {
            fallback.push(entry);
        }
    }

    let mut candidates = if preferred.is_empty() {
        fallback
    } else {
        preferred
    };
    candidates.sort();

    // Keep deterministic ordering but collapse exact content duplicates.
    let mut unique = Vec::new();
    let mut seen_contents = HashSet::new();
    for candidate in candidates {
        let source = normalize_external_mutant_source(&std::fs::read_to_string(&candidate)?);
        if seen_contents.insert(source) {
            unique.push(candidate);
        }
    }

    if unique.is_empty() {
        return Err(ApiError::ProjectError(format!(
            "no mutant files for '{}' were found in '{}'",
            base_path.display(),
            mutants_dir.display()
        )));
    }

    Ok(unique)
}

fn walk_files_recursive(root: &Path) -> Result<Vec<PathBuf>, ApiError> {
    let mut stack = vec![root.to_path_buf()];
    let mut out = Vec::new();

    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| {
            ApiError::ProjectError(format!(
                "failed to read directory '{}': {}",
                dir.display(),
                e
            ))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| ApiError::ProjectError(e.to_string()))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }

    Ok(out)
}

#[derive(Clone, Debug)]
struct UnifiedFilePatch {
    old_path: String,
    new_path: String,
    hunks: Vec<UnifiedHunk>,
}

#[derive(Clone, Debug)]
struct UnifiedHunk {
    old_start: usize,
    old_count: usize,
    lines: Vec<String>,
}

fn collect_mutant_sources_from_cargo_mutants_output(
    base_path: &Path,
    cargo_mutants_dir: &Path,
    base_source: &str,
) -> Result<Vec<String>, ApiError> {
    let diff_root = cargo_mutants_diff_root(cargo_mutants_dir);

    let mut diff_files = walk_files_recursive(&diff_root)?
        .into_iter()
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("diff"))
        .collect::<Vec<_>>();
    diff_files.sort();
    if diff_files.is_empty() {
        return Err(ApiError::ProjectError(format!(
            "no .diff files found in '{}'",
            diff_root.display()
        )));
    }

    let base_name = base_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ApiError::ProjectError("base file name is not valid unicode".to_string()))?;
    let base_rel_to_cwd = std::env::current_dir()
        .ok()
        .and_then(|cwd| base_path.strip_prefix(cwd).ok().map(|p| p.to_path_buf()));
    let base_lines = base_source
        .lines()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let base_has_trailing_newline = base_source.ends_with('\n');

    let mut sources = Vec::new();
    let mut seen = HashSet::new();

    for diff_file in diff_files {
        let diff_text = match std::fs::read_to_string(&diff_file) {
            Ok(text) => text,
            Err(_) => continue,
        };
        let patches = match parse_unified_diff(&diff_text) {
            Ok(patches) => patches,
            Err(_) => continue,
        };
        let patch = match select_patch_for_base(&patches, base_name, base_rel_to_cwd.as_deref()) {
            Some(patch) => patch,
            None => continue,
        };

        let mutated_lines = match apply_unified_patch(&base_lines, &patch.hunks) {
            Ok(lines) => lines,
            Err(_) => continue,
        };
        let mut mutated = mutated_lines.join("\n");
        if base_has_trailing_newline {
            mutated.push('\n');
        }
        mutated = normalize_external_mutant_source(&mutated);
        if mutated == base_source {
            continue;
        }
        if syn::parse_file(&mutated).is_err() {
            continue;
        }
        if seen.insert(mutated.clone()) {
            sources.push(mutated);
        }
    }

    Ok(sources)
}

fn find_cargo_project_root(base_path: &Path) -> Option<PathBuf> {
    let mut current = base_path.parent()?.to_path_buf();
    loop {
        if current.join("Cargo.toml").is_file() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

fn create_single_file_cargo_project(
    input_file: &Path,
    project_root: &Path,
) -> Result<PathBuf, ApiError> {
    std::fs::create_dir_all(project_root)?;
    let file_name = input_file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            ApiError::ProjectError("input file name is not valid unicode".to_string())
        })?;
    let copied_file = project_root.join(file_name);
    std::fs::copy(input_file, &copied_file).map_err(|e| {
        ApiError::ProjectError(format!(
            "failed to copy '{}' into synthetic Cargo project '{}': {}",
            input_file.display(),
            project_root.display(),
            e
        ))
    })?;

    let escaped_file_name = file_name.replace('\\', "\\\\").replace('"', "\\\"");
    let cargo_toml = format!(
        "[package]\nname = \"marauders_single_file\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"{}\"\n",
        escaped_file_name
    );
    std::fs::write(project_root.join("Cargo.toml"), cargo_toml)?;

    Ok(PathBuf::from(file_name))
}

fn parse_unified_diff(input: &str) -> Result<Vec<UnifiedFilePatch>, ApiError> {
    let lines = input.lines().collect::<Vec<_>>();
    let mut patches = Vec::new();
    let mut idx = 0usize;

    while idx < lines.len() {
        if !lines[idx].starts_with("--- ") {
            idx += 1;
            continue;
        }

        let old_path = parse_diff_path(lines[idx], "--- ");
        idx += 1;
        if idx >= lines.len() || !lines[idx].starts_with("+++ ") {
            continue;
        }
        let new_path = parse_diff_path(lines[idx], "+++ ");
        idx += 1;

        let mut hunks = Vec::new();
        while idx < lines.len() {
            if lines[idx].starts_with("--- ") {
                break;
            }
            if !lines[idx].starts_with("@@") {
                idx += 1;
                continue;
            }

            let (old_start, old_count) = parse_hunk_old_range(lines[idx]).ok_or_else(|| {
                ApiError::ProjectError("failed to parse unified diff hunk header".to_string())
            })?;
            idx += 1;

            let mut hunk_lines = Vec::new();
            while idx < lines.len() {
                let current = lines[idx];
                if current.starts_with("@@") || current.starts_with("--- ") {
                    break;
                }
                if current.starts_with('\\') {
                    idx += 1;
                    continue;
                }
                match current.chars().next() {
                    Some(' ') | Some('+') | Some('-') => hunk_lines.push(current.to_string()),
                    _ => {}
                }
                idx += 1;
            }

            hunks.push(UnifiedHunk {
                old_start,
                old_count,
                lines: hunk_lines,
            });
        }

        if !hunks.is_empty() {
            patches.push(UnifiedFilePatch {
                old_path,
                new_path,
                hunks,
            });
        }
    }

    Ok(patches)
}

fn parse_diff_path(line: &str, prefix: &str) -> String {
    let raw = line
        .strip_prefix(prefix)
        .unwrap_or(line)
        .split_whitespace()
        .next()
        .unwrap_or_default();
    normalize_diff_path(raw)
}

fn normalize_diff_path(path: &str) -> String {
    if path == "/dev/null" {
        return String::new();
    }
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .or_else(|| path.strip_prefix("./"))
        .unwrap_or(path)
        .to_string()
}

fn parse_hunk_old_range(header: &str) -> Option<(usize, usize)> {
    let middle = header.split("@@").nth(1)?.trim();
    let old = middle.split_whitespace().next()?;
    parse_hunk_range(old, '-')
}

fn parse_hunk_range(token: &str, prefix: char) -> Option<(usize, usize)> {
    let range = token.strip_prefix(prefix)?;
    let (start, count) = if let Some((start, count)) = range.split_once(',') {
        (start, count)
    } else {
        (range, "1")
    };
    Some((start.parse().ok()?, count.parse().ok()?))
}

fn select_patch_for_base<'a>(
    patches: &'a [UnifiedFilePatch],
    base_name: &str,
    base_rel: Option<&Path>,
) -> Option<&'a UnifiedFilePatch> {
    patches
        .iter()
        .filter_map(|patch| {
            let score = patch_match_score(patch, base_name, base_rel);
            (score > 0).then_some((score, patch))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, patch)| patch)
}

fn patch_match_score(patch: &UnifiedFilePatch, base_name: &str, base_rel: Option<&Path>) -> usize {
    [patch.old_path.as_str(), patch.new_path.as_str()]
        .into_iter()
        .map(|path| single_patch_path_score(path, base_name, base_rel))
        .max()
        .unwrap_or(0)
}

fn single_patch_path_score(path: &str, base_name: &str, base_rel: Option<&Path>) -> usize {
    if path.is_empty() {
        return 0;
    }

    let candidate = Path::new(path);
    if let Some(base_rel) = base_rel {
        if candidate == base_rel {
            return 100;
        }
        if path_ends_with(candidate, base_rel) {
            return 80;
        }
    }

    if candidate.file_name().and_then(|name| name.to_str()) == Some(base_name) {
        return 10;
    }

    0
}

fn apply_unified_patch(
    base_lines: &[String],
    hunks: &[UnifiedHunk],
) -> Result<Vec<String>, ApiError> {
    let mut out = Vec::new();
    let mut cursor = 0usize;

    for hunk in hunks {
        if hunk.old_start == 0 {
            return Err(ApiError::ProjectError(
                "invalid unified diff hunk line number".to_string(),
            ));
        }
        let target_start = hunk.old_start - 1;
        if target_start < cursor || target_start > base_lines.len() {
            return Err(ApiError::ProjectError(
                "invalid unified diff hunk range".to_string(),
            ));
        }

        out.extend_from_slice(&base_lines[cursor..target_start]);
        cursor = target_start;
        let mut consumed = 0usize;

        for line in &hunk.lines {
            let marker = line.chars().next().unwrap_or(' ');
            let body = line.get(1..).unwrap_or_default().to_string();
            match marker {
                ' ' => {
                    if base_lines.get(cursor) != Some(&body) {
                        return Err(ApiError::ProjectError(
                            "unified diff context line mismatch".to_string(),
                        ));
                    }
                    out.push(body);
                    cursor += 1;
                    consumed += 1;
                }
                '-' => {
                    if base_lines.get(cursor) != Some(&body) {
                        return Err(ApiError::ProjectError(
                            "unified diff removed line mismatch".to_string(),
                        ));
                    }
                    cursor += 1;
                    consumed += 1;
                }
                '+' => out.push(body),
                _ => {}
            }
        }

        if consumed != hunk.old_count {
            return Err(ApiError::ProjectError(
                "unified diff hunk consumed line count mismatch".to_string(),
            ));
        }
    }

    out.extend_from_slice(&base_lines[cursor..]);
    Ok(out)
}

fn path_ends_with(path: &Path, suffix: &Path) -> bool {
    let path_components = path.components().collect::<Vec<_>>();
    let suffix_components = suffix.components().collect::<Vec<_>>();
    if suffix_components.len() > path_components.len() {
        return false;
    }

    let start = path_components.len() - suffix_components.len();
    path_components[start..] == suffix_components[..]
}

fn cargo_mutants_diff_root(cargo_mutants_dir: &Path) -> PathBuf {
    let direct = cargo_mutants_dir.join("diff");
    if direct.is_dir() {
        return direct;
    }

    let nested_once = cargo_mutants_dir.join("mutants.out").join("diff");
    if nested_once.is_dir() {
        return nested_once;
    }

    let nested_twice = cargo_mutants_dir
        .join("mutants.out")
        .join("mutants.out")
        .join("diff");
    if nested_twice.is_dir() {
        return nested_twice;
    }

    cargo_mutants_dir.to_path_buf()
}

fn copy_cargo_mutants_diffs_to_workdir(
    cargo_mutants_dir: &Path,
) -> Result<Option<PathBuf>, ApiError> {
    let workdir = std::env::current_dir().map_err(|e| {
        ApiError::ProjectError(format!("failed to determine current directory: {}", e))
    })?;
    copy_cargo_mutants_diffs_to_dir(cargo_mutants_dir, &workdir)
}

fn copy_cargo_mutants_diffs_to_dir(
    cargo_mutants_dir: &Path,
    destination_root: &Path,
) -> Result<Option<PathBuf>, ApiError> {
    let diff_root = cargo_mutants_diff_root(cargo_mutants_dir);
    let mut diff_files = walk_files_recursive(&diff_root)?
        .into_iter()
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("diff"))
        .collect::<Vec<_>>();
    diff_files.sort();
    if diff_files.is_empty() {
        return Ok(None);
    }

    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let mut out = destination_root.join("diffs");
    if out.exists() {
        out = destination_root.join(format!("diffs_{pid}_{nanos}"));
    }
    std::fs::create_dir_all(&out)?;

    for file in diff_files {
        let rel = file.strip_prefix(&diff_root).map_err(|_| {
            ApiError::ProjectError(format!(
                "failed to relativize diff file '{}' to '{}'",
                file.display(),
                diff_root.display()
            ))
        })?;
        let destination = out.join(rel);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&file, &destination).map_err(|e| {
            ApiError::ProjectError(format!(
                "failed to copy diff '{}' to '{}': {}",
                file.display(),
                destination.display(),
                e
            ))
        })?;
    }

    Ok(Some(out))
}

fn normalize_external_mutant_source(input: &str) -> String {
    // cargo-mutants may annotate changed sites with this inline comment marker.
    // Strip it so generated Marauders block comments stay syntactically valid.
    const MARKER: &str = "/* ~ changed by cargo-mutants ~ */";
    if input.contains(MARKER) {
        input.replace(MARKER, "")
    } else {
        input.to_string()
    }
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

    #[test]
    fn test_convert_file_rust_functional() {
        let original = r#"
fn insert(k: i32, k2: i32) -> i32 {
    /*| insert */
    if k < k2 { 1 } else { 2 }
    /*|| insert_1 */
    /*|
    10
    */
    /* |*/
}

fn union_(l: i32, r: i32) -> i32 {
    match (l, r) {
        (0, r) => r,
        (l, 0) => l,
        /*| union */
        (l, r) => { l + r }
        /*|| union_6 */
        /*|
        (l, r) => { l - r }
        */
        /* |*/
    }
}
"#;
        let tmp =
            std::env::temp_dir().join(format!("marauders_convert_{}_bst.rs", std::process::id()));
        std::fs::write(&tmp, original).unwrap();

        let result = convert_file(&tmp, ConversionTarget::RustFunctional).unwrap();
        assert_eq!(result, tmp);

        let converted = std::fs::read_to_string(&tmp).unwrap();
        assert!(converted.contains("match () {"));
        assert!(
            converted.contains(r#"matches!(std::env::var("M_insert_1").as_deref(), Ok("active"))"#)
        );
        assert!(
            converted.contains(
                r#"if matches!(std::env::var("M_union_6").as_deref(), Ok("active")) => {"#
            ) || converted.contains(
                r#"if matches!(std::env::var("M_union_6").as_deref(), Ok("active")) => { l - r }"#
            )
        );

        let _ = convert_file(&tmp, ConversionTarget::RustComment).unwrap();
        let roundtrip = std::fs::read_to_string(&tmp).unwrap();
        assert!(roundtrip.contains("/*| insert */"));
        assert!(roundtrip.contains("/*|| insert_1 */"));
        assert!(roundtrip.contains("/*| union */"));
        assert!(roundtrip.contains("/*|| union_6 */"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_convert_file_preprocessor_roundtrip() {
        let original = r#"
fn calc(a: i32, b: i32) -> i32 {
    /*| add [arith] */
    a + b
    /*|| add_1 */
    /*|
    a - b
    */
    /*|| add_2 */
    /*|
    a * b
    */
    /* |*/
}
"#;
        let tmp = std::env::temp_dir().join(format!(
            "marauders_convert_{}_preprocessor.rs",
            std::process::id()
        ));
        std::fs::write(&tmp, original).unwrap();

        let result = convert_file(&tmp, ConversionTarget::Preprocessor).unwrap();
        assert_eq!(result, tmp);

        let converted = std::fs::read_to_string(&tmp).unwrap();
        assert!(converted.contains("#if defined(M_add_1)"));
        assert!(converted.contains("#elif defined(M_add_2)"));
        assert!(converted.contains("#else"));
        assert!(converted.contains("#endif"));
        assert!(converted.contains("marauders:variation=add;tags=arith"));

        let _ = convert_file(&tmp, ConversionTarget::Comment).unwrap();
        let roundtrip = std::fs::read_to_string(&tmp).unwrap();
        assert!(roundtrip.contains("/*| add [arith] */"));
        assert!(roundtrip.contains("/*|| add_1 */"));
        assert!(roundtrip.contains("/*|| add_2 */"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_convert_file_patch_roundtrip() {
        let original = r#"
fn calc(a: i32, b: i32) -> i32 {
    /*| add [arith] */
    a + b
    /*|| add_1 */
    /*|
    a - b
    */
    /*|| add_2 */
    /*|
    a * b
    */
    /* |*/
}
"#;
        let tmp =
            std::env::temp_dir().join(format!("marauders_convert_{}_patch.rs", std::process::id()));
        std::fs::write(&tmp, original).unwrap();

        let result = convert_file(&tmp, ConversionTarget::Patch).unwrap();
        assert_ne!(result, tmp);
        assert_eq!(
            result.file_name().and_then(|name| name.to_str()),
            Some("manifest.toml")
        );
        assert!(result
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .ends_with(".patches"));

        // Source file is kept as the base program.
        let base_after_convert = std::fs::read_to_string(&tmp).unwrap();
        assert!(base_after_convert.contains("fn calc(a: i32, b: i32) -> i32 {"));
        assert!(!base_after_convert.contains("/*| add [arith] */"));
        assert!(base_after_convert.contains("a + b"));

        let manifest = std::fs::read_to_string(&result).unwrap();
        assert!(manifest.contains("format = \"marauders_patch_bundle\""));
        assert!(manifest.contains("tags = [\"arith\"]"));
        assert!(!manifest.contains("base ="));

        let bundle_dir = result.parent().unwrap();
        let mut patch_files = Vec::new();
        for variation_entry in std::fs::read_dir(bundle_dir).unwrap() {
            let variation_entry = variation_entry.unwrap();
            if !variation_entry.file_type().unwrap().is_dir() {
                continue;
            }
            for patch_entry in std::fs::read_dir(variation_entry.path()).unwrap() {
                let patch_entry = patch_entry.unwrap();
                if patch_entry.path().extension().and_then(|ext| ext.to_str()) == Some("patch") {
                    patch_files.push(patch_entry.path());
                }
            }
        }
        assert!(!patch_files.is_empty());
        let first_patch = std::fs::read_to_string(&patch_files[0]).unwrap();
        assert!(first_patch.contains("@@ -3,1 +3,1 @@"));

        let restored_path = convert_file(&result, ConversionTarget::Comment).unwrap();
        assert_eq!(restored_path, tmp);
        let roundtrip = std::fs::read_to_string(&tmp).unwrap();
        assert!(roundtrip.contains("/*| add [arith] */"));
        assert!(roundtrip.contains("/*|| add_1 */"));
        assert!(roundtrip.contains("/*|| add_2 */"));

        let _ = std::fs::remove_file(&tmp);
        if let Some(bundle_dir) = result.parent() {
            let _ = std::fs::remove_dir_all(bundle_dir);
        }
    }

    #[test]
    fn test_convert_file_match_replace_roundtrip() {
        let original = r#"
fn calc(a: i32, b: i32) -> i32 {
    /*| add [arith] */
    a + b
    /*|| add_1 */
    /*|
    a - b
    */
    /*|| add_2 */
    /*|
    a * b
    */
    /* |*/
}
"#;
        let tmp = std::env::temp_dir().join(format!(
            "marauders_convert_{}_match_replace.rs",
            std::process::id()
        ));
        std::fs::write(&tmp, original).unwrap();

        let result = convert_file(&tmp, ConversionTarget::MatchReplace).unwrap();
        assert_ne!(result, tmp);
        assert!(result.to_string_lossy().ends_with(".match_replace.json"));

        // Source file is kept as the base program.
        let base_after_convert = std::fs::read_to_string(&tmp).unwrap();
        assert!(base_after_convert.contains("fn calc(a: i32, b: i32) -> i32 {"));
        assert!(!base_after_convert.contains("/*| add [arith] */"));
        assert!(base_after_convert.contains("a + b"));

        let converted = std::fs::read_to_string(&result).unwrap();
        assert!(!converted.contains("\"format\""));
        assert!(!converted.contains("\"base\""));
        assert!(converted.contains("\"replacement\": \"    a - b\""));
        assert!(converted.contains(&format!("\"scope\": \"{}:3\"", tmp.to_string_lossy())));
        assert!(converted.contains("\"match\": \"    a + b\""));

        let restored_path = convert_file(&result, ConversionTarget::Comment).unwrap();
        assert_eq!(restored_path, tmp);
        let roundtrip = std::fs::read_to_string(&tmp).unwrap();
        assert!(roundtrip.contains("fn calc(a: i32, b: i32) -> i32 {"));
        assert!(roundtrip.contains("/*| add [arith] */"));
        assert!(roundtrip.contains("/*|| add_1 */"));
        assert!(roundtrip.contains("/*|| add_2 */"));

        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&result);
    }

    #[test]
    fn test_import_rust_mutants() {
        let base = r#"
fn calc(a: i32, b: i32) -> i32 {
    a + b
}
"#;
        let mutant_1 = r#"
fn calc(a: i32, b: i32) -> i32 {
    a - b
}
"#;
        let mutant_2 = r#"
fn calc(a: i32, b: i32) -> i32 {
    a * b
}
"#;

        let pid = std::process::id();
        let base_path = std::env::temp_dir().join(format!("marauders_import_base_{pid}.rs"));
        let mutant_1_path = std::env::temp_dir().join(format!("marauders_import_mutant1_{pid}.rs"));
        let mutant_2_path = std::env::temp_dir().join(format!("marauders_import_mutant2_{pid}.rs"));
        let out_path = std::env::temp_dir().join(format!("marauders_import_out_{pid}.rs"));

        std::fs::write(&base_path, base).unwrap();
        std::fs::write(&mutant_1_path, mutant_1).unwrap();
        std::fs::write(&mutant_2_path, mutant_2).unwrap();

        let result = import_rust_mutants(
            &base_path,
            &[mutant_1_path.clone(), mutant_2_path.clone()],
            Some(&out_path),
            "tool",
        )
        .unwrap();
        assert_eq!(result, out_path);

        let imported = std::fs::read_to_string(&out_path).unwrap();
        assert!(imported.contains("/*| tool_1 */"));
        assert!(imported.contains("/*|| tool_1_1 */"));
        assert!(imported.contains("/*|| tool_1_2 */"));

        let _ = std::fs::remove_file(&base_path);
        let _ = std::fs::remove_file(&mutant_1_path);
        let _ = std::fs::remove_file(&mutant_2_path);
        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn test_collect_rust_mutants_from_dir() {
        let pid = std::process::id();
        let root = std::env::temp_dir().join(format!("marauders_collect_mutants_{pid}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("project/src")).unwrap();
        std::fs::create_dir_all(root.join("mutants/a/project/src")).unwrap();
        std::fs::create_dir_all(root.join("mutants/b/project/src")).unwrap();
        std::fs::create_dir_all(root.join("mutants/c/other")).unwrap();

        let base_path = root.join("project/src/calc.rs");
        let base = "fn calc(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
        let mutant_1 = "fn calc(a: i32, b: i32) -> i32 {\n    a - b\n}\n";
        let mutant_2 = "fn calc(a: i32, b: i32) -> i32 {\n    a * b\n}\n";

        std::fs::write(&base_path, base).unwrap();
        std::fs::write(root.join("mutants/a/project/src/calc.rs"), mutant_1).unwrap();
        std::fs::write(root.join("mutants/b/project/src/calc.rs"), mutant_2).unwrap();
        std::fs::write(root.join("mutants/c/other/calc.rs"), "not rust").unwrap();

        let found = collect_rust_mutants_from_dir(&base_path, &root.join("mutants")).unwrap();
        assert_eq!(found.len(), 2);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_import_rust_mutants_from_cargo_mutants_output() {
        let pid = std::process::id();
        let root = std::env::temp_dir().join(format!("marauders_cargo_mutants_{pid}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("project/src")).unwrap();
        std::fs::create_dir_all(root.join("mutants.out/diff")).unwrap();

        let base_path = root.join("project/src/calc.rs");
        let out_path = root.join("project/src/calc_imported.rs");
        std::fs::write(
            &base_path,
            "fn calc(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
        )
        .unwrap();

        let diff_1 = r#"--- a/project/src/calc.rs
+++ b/project/src/calc.rs
@@ -1,3 +1,3 @@
 fn calc(a: i32, b: i32) -> i32 {
-    a + b
+    a - b
 }
"#;
        let diff_2 = r#"--- a/project/src/calc.rs
+++ b/project/src/calc.rs
@@ -1,3 +1,3 @@
 fn calc(a: i32, b: i32) -> i32 {
-    a + b
+    a * b
 }
"#;
        std::fs::write(root.join("mutants.out/diff/m1.diff"), diff_1).unwrap();
        std::fs::write(root.join("mutants.out/diff/m2.diff"), diff_2).unwrap();

        let result = import_rust_mutants_from_cargo_mutants_output(
            &base_path,
            &root.join("mutants.out"),
            Some(&out_path),
            "cargo",
        )
        .unwrap();
        assert_eq!(result, out_path);

        let imported = std::fs::read_to_string(&out_path).unwrap();
        assert!(imported.contains("/*| cargo_1 */"));
        assert!(imported.contains("/*|| cargo_1_1 */"));
        assert!(imported.contains("/*|| cargo_1_2 */"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_import_rust_mutants_from_nested_cargo_mutants_layout() {
        let pid = std::process::id();
        let root = std::env::temp_dir().join(format!("marauders_nested_layout_{pid}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("project/src")).unwrap();
        std::fs::create_dir_all(root.join("project/mutants.out/mutants.out/diff")).unwrap();

        let base_path = root.join("project/src/main.rs");
        let out_path = root.join("project/src/main_imported.rs");
        std::fs::write(
            &base_path,
            "fn calc(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
        )
        .unwrap();
        let diff = r#"--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
 fn calc(a: i32, b: i32) -> i32 {
-    a + b
+    a - b
 }
"#;
        std::fs::write(
            root.join("project/mutants.out/mutants.out/diff/m1.diff"),
            diff,
        )
        .unwrap();

        let result = import_rust_mutants_from_cargo_mutants_output(
            &base_path,
            &root.join("project/mutants.out"),
            Some(&out_path),
            "nested",
        )
        .unwrap();
        assert_eq!(result, out_path);

        let imported = std::fs::read_to_string(&out_path).unwrap();
        assert!(imported.contains("/*| nested_1 */"));
        assert!(imported.contains("/*|| nested_1_1 */"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_find_cargo_project_root() {
        let pid = std::process::id();
        let root = std::env::temp_dir().join(format!("marauders_find_root_{pid}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crate/src/nested")).unwrap();
        std::fs::write(
            root.join("crate/Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n",
        )
        .unwrap();
        let file = root.join("crate/src/nested/file.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();

        let found = find_cargo_project_root(&file).unwrap();
        assert_eq!(found, root.join("crate"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_create_single_file_cargo_project() {
        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("marauders_single_file_test_{pid}_{nanos}"));
        let input = root.join("input.rs");
        let synthetic = root.join("synthetic");

        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&input, "pub fn add(a: i32, b: i32) -> i32 { a + b }\n").unwrap();

        let rel = create_single_file_cargo_project(&input, &synthetic).unwrap();
        assert_eq!(rel, PathBuf::from("input.rs"));
        assert!(synthetic.join("Cargo.toml").is_file());
        assert!(synthetic.join("input.rs").is_file());
        let cargo_toml = std::fs::read_to_string(synthetic.join("Cargo.toml")).unwrap();
        assert!(cargo_toml.contains("path = \"input.rs\""));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_copy_cargo_mutants_diffs_to_dir() {
        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("marauders_copy_diffs_{pid}_{nanos}"));
        let cargo_mutants_dir = root.join("mutants.out");
        let destination_root = root.join("workspace");
        std::fs::create_dir_all(cargo_mutants_dir.join("diff/nested")).unwrap();
        std::fs::create_dir_all(&destination_root).unwrap();
        std::fs::write(cargo_mutants_dir.join("diff/1.diff"), "--- a/x\n+++ b/x\n").unwrap();
        std::fs::write(
            cargo_mutants_dir.join("diff/nested/2.diff"),
            "--- a/y\n+++ b/y\n",
        )
        .unwrap();

        let copied =
            copy_cargo_mutants_diffs_to_dir(&cargo_mutants_dir, &destination_root).unwrap();
        let copied = copied.unwrap();
        assert!(copied.starts_with(&destination_root));
        assert!(copied.join("1.diff").is_file());
        assert!(copied.join("nested/2.diff").is_file());

        let _ = std::fs::remove_dir_all(root);
    }
}
