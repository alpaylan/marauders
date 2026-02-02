//! CLI command implementations.
//!
//! These functions are thin wrappers around the library API that handle
//! printing and user-facing output. They are used by the CLI binary
//! but not exported from the library.

use std::path::Path;

use crate::api::{self, ApiError};
use crate::Project;

pub(crate) fn run_list_command(path: &Path, pattern: Option<&str>) -> anyhow::Result<()> {
    let project = Project::new(path, pattern)?;

    for info in api::list_variations(&project) {
        let name = info.name.as_deref().unwrap_or("anonymous");
        let active = if info.active == 0 {
            "base".to_string()
        } else {
            info.variants[info.active - 1].clone()
        };
        println!(
            "{}:{} (name: {}, active: {}, variants: {:?}, tags: {:?})",
            info.path.to_string_lossy(),
            info.line,
            name,
            active,
            info.variants,
            info.tags
        );
    }

    Ok(())
}

pub(crate) fn run_set_command(
    path: &Path,
    variant: &str,
    pattern: Option<&str>,
) -> anyhow::Result<()> {
    let mut project = Project::new(path, pattern)?;

    match api::set_variant(&mut project, variant) {
        Ok(result) => {
            log::info!(
                "set variant '{}' in {} (was index {}, now index {})",
                variant,
                result.file.to_string_lossy(),
                result.previous_active,
                result.new_active
            );
            Ok(())
        }
        Err(ApiError::VariantNotFound { variant, available }) => {
            log::error!(
                "variant '{}' not found, possible variants are (\n{}\n)",
                variant,
                available
                    .iter()
                    .map(|v| format!("\t'{}'", v))
                    .collect::<Vec<String>>()
                    .join(",\n")
            );
            Ok(())
        }
        Err(ApiError::AlreadyActive { variant }) => {
            log::warn!("variant '{}' is already active", variant);
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("{}", e)),
    }
}

pub(crate) fn run_unset_command(path: &Path, variant: &str) -> anyhow::Result<()> {
    let mut project = Project::new(path, None)?;

    match api::unset_variant(&mut project, variant) {
        Ok(result) => {
            log::info!(
                "unset variant in {} (was index {}, now base)",
                result.file.to_string_lossy(),
                result.previous_active
            );
            Ok(())
        }
        Err(ApiError::VariantNotFound { variant, available }) => {
            log::error!(
                "variant '{}' not found, possible variants are (\n{}\n)",
                variant,
                available
                    .iter()
                    .map(|v| format!("\t'{}'", v))
                    .collect::<Vec<String>>()
                    .join(",\n")
            );
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("{}", e)),
    }
}

pub(crate) fn run_reset_command(path: &Path) -> anyhow::Result<()> {
    let mut project = Project::new(path, None)?;

    let results = api::reset_all(&mut project)?;

    if results.is_empty() {
        log::info!("all variations already at base");
    } else {
        log::info!("reset {} variation(s) to base", results.len());
    }

    Ok(())
}

pub(crate) fn run_init_command(
    path: &Path,
    language: &str,
    use_gitignore: bool,
) -> anyhow::Result<()> {
    let config_path = api::init_project(path, language, use_gitignore)?;

    log::info!("project initialized at '{}'", config_path.to_string_lossy());

    Ok(())
}
