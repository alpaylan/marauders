use std::path::{Path, PathBuf};

use crate::{code::Code, languages::Language};

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

impl Project {
    pub(crate) fn new(path: &Path, pattern: Option<&str>) -> anyhow::Result<Self> {
        let root = PathBuf::from(path);

        // Check that the path is a directory
        if !root.is_dir() {
            anyhow::bail!("'{}' is not a directory", path.to_string_lossy());
        }

        let files = glob::glob(path.join(pattern.unwrap_or("**/*")).to_str().unwrap())
            .expect("Failed to read glob pattern")
            .filter_map(Result::ok)
            .filter_map(|path| {
                let code = Code::from_file(&path);
                match code {
                    Ok(code) => Some(ProjectFile { path, code }),
                    Err(err) => {
                        log::error!("could not read file '{}': {}", path.to_string_lossy(), err);
                        None
                    }
                }
            })
            .collect();
        Ok(Project { root, files })
    }

    pub(crate) fn with_language(path: &Path, lang: &Language) -> anyhow::Result<Self> {
        Self::new(
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
        let project = Project::new(Path::new("test"), None).unwrap();
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
        let project = Project::new(Path::new("."), None).unwrap();
        assert_eq!(project.root, PathBuf::from("."));
        let file_paths = project
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>();
        println!("{:?}", file_paths);
        assert!(file_paths.contains(&PathBuf::from("test/BST.v")));
        assert!(file_paths.contains(&PathBuf::from("src/syntax/comment.rs")));
    }

    #[test]
    fn test_project_lang() {
        let project = Project::with_language(Path::new("."), &Language::Coq).unwrap();
        assert_eq!(project.root, PathBuf::from("."));
        let file_paths = project
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>();
        assert!(file_paths.contains(&PathBuf::from("test/BST.v")));
        assert!(file_paths.contains(&PathBuf::from("test/STLC.v")));
    }
}
