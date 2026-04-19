use std::{
    collections::HashMap,
    fs::{self, FileType},
    path::{Path, PathBuf},
    process::Output,
};

use anyhow::Context;
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use serde::{Deserialize, Serialize};

use crate::{
    code::Code,
    languages::{CustomLanguage, Language},
    SpanContent,
};

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub files: Vec<ProjectFile>,
    pub config: Option<ProjectConfig>,
    pub parse_errors: Vec<String>,
    /// Standalone unified-diff patches discovered under `<root>/patches/*.patch`.
    /// Each entry is one variation whose sole variant is the patch file stem;
    /// activation invokes `git apply` on the patch. Applied-state is tracked
    /// in `<root>/.marauders/active_patch`, not in source files.
    pub patches: Vec<PatchVariation>,
}

#[derive(Debug)]
pub struct ProjectFile {
    pub path: PathBuf,
    pub code: Code,
}

/// A mutation materialized as a standalone git patch under `<root>/patches/`.
/// Unlike comment/preprocessor/match-replace variations that live inside a
/// source file, a patch variation is activated by applying the diff and
/// deactivated by reverse-applying it.
#[derive(Debug, Clone)]
pub struct PatchVariation {
    /// Variant name — the patch file stem (e.g. `foo` for `patches/foo.patch`).
    pub name: String,
    /// Absolute path to the `.patch` file.
    pub patch_file: PathBuf,
}

const ACTIVE_PATCH_FILE: &str = ".marauders/active_patch";

fn load_patch_variations(root: &Path) -> Vec<PatchVariation> {
    let patches_dir = root.join("patches");
    if !patches_dir.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let entries = match fs::read_dir(&patches_dir) {
        Ok(entries) => entries,
        Err(err) => {
            log::warn!(
                "could not read patches dir '{}': {}",
                patches_dir.display(),
                err
            );
            return Vec::new();
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("patch") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        out.push(PatchVariation {
            name: name.to_string(),
            patch_file: path,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Project configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// List of languages that should be analyzed for mutations
    pub languages: Vec<Language>,
    /// List of glob strings to ignore
    pub ignore: Vec<String>,
    /// Whether to ignore files based on .gitignore
    pub use_gitignore: bool,
    /// Custom languages outside of the standart set
    pub custom_languages: Vec<CustomLanguage>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        ProjectConfig {
            languages: vec![],
            ignore: vec![],
            use_gitignore: true,
            custom_languages: vec![],
        }
    }
}

fn file_extension(path: &Path) -> Option<&str> {
    path.extension().and_then(|ext| ext.to_str())
}

fn is_supported_source_path(path: &Path, custom_languages: &Vec<CustomLanguage>) -> bool {
    let Some(ext) = file_extension(path) else {
        return false;
    };
    Language::extension_to_language(ext, custom_languages).is_some()
}

fn is_allowed_path_for_config(path: &Path, config: &ProjectConfig) -> bool {
    let Some(ext) = file_extension(path) else {
        return false;
    };

    config
        .languages
        .iter()
        .any(|lang| lang.file_extension() == ext)
        || config
            .custom_languages
            .iter()
            .any(|custom| custom.extension == ext)
}

impl Project {
    pub fn new(path: &Path, pattern: Option<&str>) -> anyhow::Result<Self> {
        let cfg = if let Ok(cfg_path) = std::env::var("MARAUDER_CONFIG") {
            fs::read_to_string(cfg_path).ok()
        } else {
            fs::read_to_string(path.join("marauder.toml")).ok()
        };

        if let Some(cfg) = cfg {
            log::info!("found project config at '{}'", path.to_string_lossy());
            if pattern.is_some() {
                // todo: allow advancing the pattern to the project config
                log::warn!("ignoring pattern, project config found");
            }
            let project_config: ProjectConfig = toml::from_str(&cfg)?;
            Project::with_config(path, project_config)
        } else {
            Project::with_pattern(path, pattern)
        }
    }

    pub fn with_pattern(path: &Path, pattern: Option<&str>) -> anyhow::Result<Self> {
        let root = PathBuf::from(path);

        let mut overrides = OverrideBuilder::new(path);
        let empty_custom_languages = vec![];

        if let Some(s) = pattern {
            overrides.add(s)?;
        }

        let walk = WalkBuilder::new(path).overrides(overrides.build()?).build();

        let mut parse_errors = Vec::new();
        let files = walk
            .filter_map(|entry| {
                let entry = entry.unwrap();
                if entry
                    .file_type()
                    .map(|f| FileType::is_dir(&f))
                    .unwrap_or(false)
                {
                    return None;
                }

                if !is_supported_source_path(entry.path(), &empty_custom_languages) {
                    return None;
                }

                let code = Code::from_file(entry.path(), &empty_custom_languages);
                match code {
                    Ok(code) => Some(ProjectFile {
                        path: entry.path().to_path_buf(),
                        code,
                    }),
                    Err(err) => {
                        let message = format!(
                            "could not read file '{}': {}",
                            entry.path().to_string_lossy(),
                            err
                        );
                        log::warn!("{message}");
                        parse_errors.push(message);
                        None
                    }
                }
            })
            .collect();

        let patches = load_patch_variations(&root);

        Ok(Project {
            root,
            files,
            config: None,
            parse_errors,
            patches,
        })
    }

    pub fn with_config(path: &Path, config: ProjectConfig) -> anyhow::Result<Self> {
        let root = PathBuf::from(path);

        let mut overrides = OverrideBuilder::new(path);

        // Add language patterns
        for lang in &config.languages {
            overrides.add(format!("**/*.{}", lang.file_extension()).as_str())?;
        }
        // Add custom language patterns
        for custom in &config.custom_languages {
            overrides.add(format!("**/*.{}", custom.extension).as_str())?;
        }

        // Add ignore patterns
        for ignore in &config.ignore {
            overrides.add(format!("!{ignore}").as_str())?;
        }

        let walk = WalkBuilder::new(path)
            .git_ignore(config.use_gitignore)
            .overrides(overrides.build()?)
            .build();

        let mut files = Vec::new();
        let mut parse_errors = Vec::new();
        for entry in walk {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                continue;
            }
            if !is_allowed_path_for_config(entry.path(), &config) {
                continue;
            }
            log::trace!("found file: {}", entry.path().to_string_lossy());
            let code = Code::from_file(entry.path(), &config.custom_languages);
            match code {
                Ok(code) => files.push(ProjectFile {
                    path: entry.path().to_path_buf(),
                    code,
                }),
                Err(err) => {
                    let message = format!(
                        "could not read file '{}': {}",
                        entry.path().to_string_lossy(),
                        err
                    );
                    log::error!("{message}");
                    parse_errors.push(message);
                }
            }
        }

        let patches = load_patch_variations(&root);

        Ok(Project {
            root,
            files,
            config: Some(config),
            parse_errors,
            patches,
        })
    }

    pub fn with_language(path: &Path, lang: &Language) -> anyhow::Result<Self> {
        Self::with_pattern(
            path,
            Some(format!("**/*.{}", lang.file_extension()).as_str()),
        )
    }
}

impl Project {
    /// Returns the name of the currently-active patch variation, if any.
    /// Reads `<root>/.marauders/active_patch`; returns `None` if the file is
    /// missing, empty, or unreadable.
    pub fn active_patch(&self) -> Option<String> {
        let path = self.root.join(ACTIVE_PATCH_FILE);
        let text = fs::read_to_string(&path).ok()?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Writes (or clears) the active-patch state file. Creates the
    /// `<root>/.marauders/` directory on demand.
    pub fn set_active_patch(&self, variant: Option<&str>) -> std::io::Result<()> {
        let state_dir = self.root.join(".marauders");
        let path = state_dir.join("active_patch");
        match variant {
            Some(name) => {
                fs::create_dir_all(&state_dir)?;
                fs::write(&path, name)
            }
            None => match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err),
            },
        }
    }
}

impl Project {
    /// Returns the list of active variants in the project
    pub fn active_variants(&self) -> Vec<&str> {
        let mut variants = Vec::new();
        for file in &self.files {
            for span in &file.code.spans {
                if let SpanContent::Variation(v) = &span.content {
                    if v.active != 0 {
                        variants.push(v.variants[v.active - 1].name.as_str());
                    }
                }
            }
        }
        variants
    }

    /// Returns a hashmap of tag names, to a list of variations that have that tag
    pub fn tag_map(&self) -> HashMap<String, Vec<String>> {
        let mut tag_map = HashMap::new();
        for file in &self.files {
            for span in &file.code.spans {
                if let SpanContent::Variation(v) = &span.content {
                    if let Some(name) = &v.name {
                        for tag in &v.tags {
                            let tag = tag.to_string();
                            let variations = tag_map.entry(tag).or_insert(vec![]);
                            variations.push(name.clone());
                        }
                    }
                }
            }
        }
        tag_map
    }

    /// Returns a hashmap of variation names, to the list of variants in that variation
    pub fn variation_map(&self) -> HashMap<String, Vec<String>> {
        let mut variation_map = HashMap::new();
        for file in &self.files {
            for span in &file.code.spans {
                if let SpanContent::Variation(v) = &span.content {
                    // Only add variations with a name
                    if let Some(name) = &v.name {
                        let variants = variation_map.entry(name.clone()).or_insert(vec![]);
                        for variant in &v.variants {
                            variants.push(variant.name.clone());
                        }
                    }
                }
            }
        }
        variation_map
    }

    /// Returns a list of all variants in the project
    pub fn all_variants(&self) -> Vec<String> {
        let mut variants = vec![];
        for file in &self.files {
            for span in &file.code.spans {
                if let SpanContent::Variation(v) = &span.content {
                    for variant in &v.variants {
                        variants.push(variant.name.clone());
                    }
                }
            }
        }
        variants
    }

    /// Sets the active variant
    pub fn set(&mut self, variant: &str) -> anyhow::Result<()> {
        let mut found = false;
        let mut variants = vec![];
        for file in self.files.iter_mut() {
            let code = &mut file.code;
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

    /// Sets the active variants for a test
    pub fn set_many(&mut self, test: &Vec<String>) -> anyhow::Result<()> {
        for variant in test {
            self.set(variant)?;
        }
        Ok(())
    }

    /// Runs a command at the project root
    pub fn run(&self, command: &str) -> anyhow::Result<Output> {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&self.root)
            .output()
            .context("failed to run command")
    }

    /// Resets a project to the base
    pub fn reset(&mut self) -> anyhow::Result<()> {
        for file in self.files.iter_mut() {
            let mut file_changed = false;
            file.code.spans.iter_mut().for_each(|span| {
                if let SpanContent::Variation(v) = &mut span.content {
                    if v.active != 0 {
                        v.active = 0;
                        v.activate_base();
                        file_changed = true;
                    }
                }
            });

            if file_changed {
                file.code.save_to_file(&file.path)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_new() {
        let project = Project::with_pattern(Path::new("test"), None).unwrap();
        assert_eq!(project.root, PathBuf::from("test"));
        let file_paths = project
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>();
        // Should find all supported language files in test directory
        assert!(file_paths.contains(&PathBuf::from("test/rocq/BST.v")));
        assert!(file_paths.contains(&PathBuf::from("test/rocq/RBT.v")));
        assert!(file_paths.contains(&PathBuf::from("test/rocq/STLC.v")));
        assert!(file_paths.contains(&PathBuf::from("test/racket/BST.rkt")));
        assert!(file_paths.contains(&PathBuf::from("test/python/bst.py")));
        assert!(file_paths.contains(&PathBuf::from("test/haskell/BST.hs")));
        let rust_fixture = PathBuf::from("test/rust/bst.rs");
        let expected_min = if rust_fixture.exists() { 7 } else { 6 };
        if rust_fixture.exists() {
            assert!(file_paths.contains(&rust_fixture));
        }
        assert!(project.files.len() >= expected_min);
    }

    #[test]
    fn test_project_recursive() {
        let project = Project::with_pattern(Path::new("."), Some("!src/lib.rs")).unwrap();
        assert_eq!(project.root, PathBuf::from("."));
        let file_paths = project
            .files
            .iter()
            .map(|f| f.path.clone().canonicalize().unwrap())
            .collect::<Vec<_>>();

        println!("{:?}", file_paths);
        assert!(file_paths.contains(&PathBuf::from("test/rocq/BST.v").canonicalize().unwrap()));
        assert!(file_paths.contains(&PathBuf::from("src/syntax/mod.rs").canonicalize().unwrap()));
    }

    #[test]
    fn test_project_lang() {
        let project = Project::with_language(Path::new("."), &Language::Rocq).unwrap();
        assert_eq!(project.root, PathBuf::from("."));
        let file_paths = project
            .files
            .iter()
            .map(|f| f.path.clone().canonicalize().unwrap())
            .collect::<Vec<_>>();
        assert!(file_paths.contains(&PathBuf::from("test/rocq/BST.v").canonicalize().unwrap()));
        assert!(file_paths.contains(&PathBuf::from("test/rocq/STLC.v").canonicalize().unwrap()));
    }

    #[test]
    fn test_project_config() {
        let config = ProjectConfig {
            languages: vec![Language::Rust],
            ignore: vec!["src/syntax".to_string(), "**/src/lib.rs".to_string()],
            use_gitignore: false,
            custom_languages: vec![],
        };
        let project = Project::with_config(Path::new("."), config).unwrap();
        assert_eq!(project.root, PathBuf::from("."));

        let file_paths = project
            .files
            .iter()
            .map(|f| f.path.clone().canonicalize().unwrap())
            .collect::<Vec<_>>();
        assert!(file_paths.contains(&PathBuf::from("src/cli.rs").canonicalize().unwrap()));
        assert!(!file_paths.contains(
            &PathBuf::from("./src/syntax/comment.rs")
                .canonicalize()
                .unwrap()
        ));
        assert!(!file_paths.contains(&PathBuf::from("test/rocq/BST.v").canonicalize().unwrap()));
    }

    #[test]
    fn test_project_with_config_ignores_non_source_files() {
        let root = std::env::temp_dir().join(format!(
            "marauders_project_config_ignore_nonsource_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::write(root.join("README.md"), "# test\n").unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();

        let config = ProjectConfig {
            languages: vec![Language::Rust],
            ignore: vec![],
            use_gitignore: false,
            custom_languages: vec![],
        };
        let project = Project::with_config(&root, config).unwrap();
        assert!(project.parse_errors.is_empty());
        assert_eq!(project.files.len(), 1);
        assert!(project.files[0].path.ends_with("main.rs"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_project_config_gitignore() {
        let config = ProjectConfig {
            languages: vec![Language::Rust],
            ignore: vec!["src/syntax".to_string(), "src/lib.rs".to_string()],
            use_gitignore: true,
            custom_languages: vec![],
        };
        let project = Project::with_config(Path::new("."), config).unwrap();
        assert_eq!(project.root, PathBuf::from("."));

        let file_paths = project
            .files
            .iter()
            .map(|f| f.path.clone().canonicalize().unwrap())
            .collect::<Vec<_>>();
        assert!(file_paths.contains(&PathBuf::from("src/cli.rs").canonicalize().unwrap()));
        assert!(!file_paths.contains(
            &PathBuf::from("./src/syntax/comment.rs")
                .canonicalize()
                .unwrap()
        ));
        assert!(!file_paths.contains(&PathBuf::from("test/rocq/BST.v").canonicalize().unwrap()));
        // todo: make this work in the CI
        // assert!(!file_paths.contains(
        //     &PathBuf::from("target/package/marauder-0.0.1/src/cli.rs")
        //         .canonicalize()
        //         .unwrap()
        // ));
    }

    #[test]
    fn test_project_config_custom_language() {
        let config = ProjectConfig {
            languages: vec![],
            ignore: vec!["src/syntax".to_string(), "src/lib.rs".to_string()],
            use_gitignore: true,
            custom_languages: vec![CustomLanguage {
                name: "Marauder".to_string(),
                extension: "rs".to_string(),
                comment_begin: "/*".to_string(),
                comment_end: "*/".to_string(),
                mutation_marker: "|".to_string(),
            }],
        };
        let project = Project::with_config(Path::new("."), config).unwrap();
        assert_eq!(project.root, PathBuf::from("."));
        let file_paths = project
            .files
            .iter()
            .map(|f| f.path.clone().canonicalize().unwrap())
            .collect::<Vec<_>>();
        assert!(file_paths.contains(&PathBuf::from("src/cli.rs").canonicalize().unwrap()));
        assert!(!file_paths.contains(
            &PathBuf::from("./src/syntax/comment.rs")
                .canonicalize()
                .unwrap()
        ));
        assert!(!file_paths.contains(&PathBuf::from("test/rocq/BST.v").canonicalize().unwrap()));
    }
}
