use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{Language, Project, ProjectConfig, SpanContent};

pub fn run_list_command(path: &Path, pattern: Option<&str>) -> anyhow::Result<()> {
    // Check if there's a project config in the path
    let project = Project::new(path, pattern)?;

    for file in project.files.iter() {
        let code = &file.code;

        for span in code.spans.iter() {
            if let SpanContent::Variation(v) = &span.content {
                println!("{}:{} {}", file.path.to_string_lossy(), span.line, v);
            }
        }
    }

    Ok(())
}

pub fn run_set_command(path: &Path, variant: &str) -> anyhow::Result<()> {
    // todo: check currently active variant, and do not set it again
    let project = Project::new(path, None)?;

    let mut found = false;
    let mut variants = vec![];
    for file in project.files.into_iter() {
        let mut code = file.code;
        if let Some((variation_index, variation)) =
            code.spans
                .iter()
                .enumerate()
                .find(|(_, v)| match &v.content {
                    SpanContent::Variation(v) => v.variants.iter().any(|v| v.name == variant),
                    _ => false,
                })
        {
            found = true;
            let variation = match &variation.content {
                SpanContent::Variation(v) => v,
                _ => unreachable!(),
            };

            let (variant_index, _) = variation
                .variants
                .iter()
                .enumerate()
                .find(|(_, v)| v.name == variant)
                .ok_or_else(|| anyhow::anyhow!("variant not found"))?;

            // Shift index by because 0 is reserved for the base code
            let variant_index = variant_index + 1;

            log::info!(
                "variant index is '{}' at '({}, {})'",
                variant_index,
                variation.name.as_deref().unwrap_or("anonymous"),
                variation_index,
            );

            code.set_active_variant(variation_index, variant_index)?;

            log::info!("active variant set to '{}'", variant);
            println!("active variant set to '{}'", variant);
        } else {
            variants.extend(
                code.get_all_variants()
                    .into_iter()
                    .map(|v| (file.path.clone(), v)),
            );
        }
    }

    if !found {
        log::error!(
            "variant '{variant}' not found, possible variants are (\n{}\n)",
            variants
                .iter()
                .map(|(path, v): &(PathBuf, String)| format!(
                    "\t'{}' at '{}'",
                    v,
                    path.to_string_lossy()
                ))
                .collect::<Vec<String>>()
                .join(",\n")
        )
    }

    Ok(())
}

pub fn run_unset_command(path: &Path, variant: &str) -> anyhow::Result<()> {
    // todo: check currently active variant, if it is not set, do not unset it
    let project = Project::new(path, None)?;

    let mut found = false;
    let mut variants = vec![];

    for file in project.files.into_iter() {
        let mut code = file.code;
        if let Some((variation_index, variation)) =
            code.spans
                .iter()
                .enumerate()
                .find(|(_, v)| match &v.content {
                    SpanContent::Variation(v) => v.variants.iter().any(|v| v.name == variant),
                    _ => false,
                })
        {
            found = true;
            let variation = match &variation.content {
                SpanContent::Variation(v) => v,
                _ => unreachable!(),
            };

            log::info!(
                "variant is '({}, {})'",
                variation.name.as_deref().unwrap_or("anonymous"),
                variation_index,
            );

            code.set_active_variant(variation_index, 0)?;

            log::info!("active variant unset");
            println!("active variant unset");
        } else {
            variants.extend(
                code.get_all_variants()
                    .into_iter()
                    .map(|v| (file.path.clone(), v)),
            );
        }
    }

    Ok(())
}

pub fn run_reset_command(path: &Path) -> anyhow::Result<()> {
    let mut project = Project::new(path, None)?;

    project.reset()?;

    log::info!("all variations reset to base");
    println!("all variations reset to base");

    Ok(())
}

pub fn run_init_command(path: &Path, language: &str, use_gitignore: bool) -> anyhow::Result<()> {
    let project_config = ProjectConfig {
        languages: Language::name_to_language(language, &vec![]).map_or(vec![], |l| vec![l]),
        custom_languages: vec![],
        ignore: vec![],
        use_gitignore,
    };

    fs::write(
        path.join("marauder.toml"),
        toml::to_string(&project_config)?,
    )?;

    log::info!("project initialized at '{}'", path.to_string_lossy());
    println!("project initialized at '{}'", path.to_string_lossy());

    Ok(())
}
