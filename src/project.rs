use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::Context;
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use serde::{Deserialize, Serialize};

use crate::{
    code::Code,
    languages::{CustomLanguage, Language},
};

#[derive(Debug)]
pub(crate) struct Project {
    pub(crate) root: PathBuf,
    pub(crate) files: Vec<ProjectFile>,
}

#[derive(Debug)]
pub(crate) struct ProjectFile {
    pub(crate) path: PathBuf,
    pub(crate) code: Code,
}

/// Project configuration
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ProjectConfig {
    /// List of languages that should be analyzed for mutations
    pub(crate) languages: Vec<Language>,
    /// List of glob strings to ignore
    pub(crate) ignore: Vec<String>,
    /// Whether to ignore files based on .gitignore
    pub(crate) use_gitignore: bool,
    /// Custom languages outside of the standart set
    pub(crate) custom_languages: Vec<CustomLanguage>,
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

impl Project {
    pub(crate) fn new(path: &Path, pattern: Option<&str>) -> anyhow::Result<Self> {
        let cfg = fs::read_to_string(path.join("marauder.toml")).ok();

        if let Some(cfg) = cfg {
            log::info!("found project config at '{}'", path.to_string_lossy());
            if pattern.is_some() {
                // todo: allow advancing the pattern to the project config
                log::warn!("ignoring pattern, project config found");
            }
            let project_config: ProjectConfig = toml::from_str(&cfg)?;
            Project::with_config(path, &project_config)
        } else {
            Project::with_pattern(path, pattern)
        }
    }

    pub(crate) fn with_pattern(path: &Path, pattern: Option<&str>) -> anyhow::Result<Self> {
        let root = PathBuf::from(path);

        let mut overrides = OverrideBuilder::new(path);

        if let Some(s) = pattern {
            overrides.add(s)?;
        }

        let walk = WalkBuilder::new(path).overrides(overrides.build()?).build();

        let files = walk
            .filter_map(|entry| {
                let entry = entry.unwrap();
                let code = Code::from_file(entry.path());
                match code {
                    Ok(code) => Some(ProjectFile {
                        path: entry.path().to_path_buf(),
                        code,
                    }),
                    Err(err) => {
                        log::error!(
                            "could not read file '{}': {}",
                            entry.path().to_string_lossy(),
                            err
                        );
                        None
                    }
                }
            })
            .collect();

        Ok(Project { root, files })
    }

    pub(crate) fn with_config(path: &Path, config: &ProjectConfig) -> anyhow::Result<Self> {
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

        let files = walk
            .filter_map(|entry| {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    return None;
                }

                let code = Code::from_file(entry.path());
                match code {
                    Ok(code) => Some(ProjectFile {
                        path: entry.path().to_path_buf(),
                        code,
                    }),
                    Err(err) => {
                        log::error!(
                            "could not read file '{}': {}",
                            entry.path().to_string_lossy(),
                            err
                        );
                        None
                    }
                }
            })
            .collect::<Vec<ProjectFile>>();

        Ok(Project { root, files })
    }

    pub(crate) fn with_language(path: &Path, lang: &Language) -> anyhow::Result<Self> {
        Self::with_pattern(
            path,
            Some(format!("**/*.{}", lang.file_extension()).as_str()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_new() {
        let project = Project::with_pattern(Path::new("test"), None).unwrap();
        assert_eq!(project.root, PathBuf::from("test"));
        assert_eq!(project.files.len(), 3);
        let file_paths = project
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>();
        assert!(file_paths.contains(&PathBuf::from("test/BST.v")));
        assert!(file_paths.contains(&PathBuf::from("test/RBT.v")));
        assert!(file_paths.contains(&PathBuf::from("test/STLC.v")));
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

        assert!(file_paths.contains(&PathBuf::from("test/BST.v").canonicalize().unwrap()));
        assert!(file_paths.contains(
            &PathBuf::from("src/syntax/comment.rs")
                .canonicalize()
                .unwrap()
        ));
    }

    #[test]
    fn test_project_lang() {
        let project = Project::with_language(Path::new("."), &Language::Coq).unwrap();
        assert_eq!(project.root, PathBuf::from("."));
        let file_paths = project
            .files
            .iter()
            .map(|f| f.path.clone().canonicalize().unwrap())
            .collect::<Vec<_>>();
        assert!(file_paths.contains(&PathBuf::from("test/BST.v").canonicalize().unwrap()));
        assert!(file_paths.contains(&PathBuf::from("test/STLC.v").canonicalize().unwrap()));
    }

    #[test]
    fn test_project_config() {
        let config = ProjectConfig {
            languages: vec![Language::Rust],
            ignore: vec!["src/syntax".to_string(), "**/src/lib.rs".to_string()],
            use_gitignore: false,
            custom_languages: vec![],
        };
        let project = Project::with_config(Path::new("."), &config).unwrap();
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
        assert!(!file_paths.contains(&PathBuf::from("test/BST.v").canonicalize().unwrap()));
    }

    #[test]
    fn test_project_config_gitignore() {
        let config = ProjectConfig {
            languages: vec![Language::Rust],
            ignore: vec!["src/syntax".to_string(), "src/lib.rs".to_string()],
            use_gitignore: true,
            custom_languages: vec![],
        };
        let project = Project::with_config(Path::new("."), &config).unwrap();
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
        assert!(!file_paths.contains(&PathBuf::from("test/BST.v").canonicalize().unwrap()));
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
        let project = Project::with_config(Path::new("."), &config).unwrap();
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
        assert!(!file_paths.contains(&PathBuf::from("test/BST.v").canonicalize().unwrap()));
    }
}
