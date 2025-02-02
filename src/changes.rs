use anyhow::{Result, ensure};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct Changes {
    base_dir_original: PathBuf,
    base_dir_modified: PathBuf,

    pub modified: Vec<PathBuf>,
    #[allow(dead_code)]
    pub removed: Vec<PathBuf>,
    #[allow(dead_code)]
    pub added: Vec<PathBuf>,
}
impl Changes {
    pub fn detect(original_dir: &Path, modified_dir: &Path) -> Result<Self> {
        read_changes(original_dir, modified_dir)
    }

    pub fn original_contents(&self, path: &Path) -> Result<String> {
        Ok(std::fs::read_to_string(self.base_dir_original.join(path))?)
    }
    pub fn modified_contents(&self, path: &Path) -> Result<String> {
        Ok(std::fs::read_to_string(self.base_dir_modified.join(path))?)
    }
}

fn read_diff_paths(dir: &Path) -> Result<BTreeSet<PathBuf>> {
    let mut paths = BTreeSet::new();
    for entry in WalkDir::new(dir) {
        let entry = entry?;

        let file_type = entry.file_type();
        ensure!(!file_type.is_symlink(), "symlinks are not supported yet");

        if file_type.is_dir() || entry.file_name() == "JJ-INSTRUCTIONS" {
            continue;
        }
        let relative = entry.path().strip_prefix(dir)?.to_owned();

        paths.insert(relative);
    }

    Ok(paths)
}

fn read_changes(original_dir: &Path, modified_dir: &Path) -> Result<Changes> {
    let original_paths = read_diff_paths(original_dir)?;
    let modified_paths = read_diff_paths(modified_dir)?;

    let modified: Vec<_> = original_paths
        .intersection(&modified_paths)
        .map(ToOwned::to_owned)
        .collect();
    let removed: Vec<_> = original_paths
        .difference(&modified_paths)
        .map(ToOwned::to_owned)
        .collect();
    let added: Vec<_> = modified_paths
        .difference(&original_paths)
        .map(ToOwned::to_owned)
        .collect();

    Ok(Changes {
        base_dir_original: original_dir.to_owned(),
        base_dir_modified: modified_dir.to_owned(),
        modified,
        removed,
        added,
    })
}
