use color_eyre::Result;
use color_eyre::eyre::{ensure, eyre};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct Changes {
    base_dir_original: PathBuf,
    base_dir_modified: PathBuf,

    pub changes: Vec<ChangeKind>,
}

pub enum ChangeKind {
    Modified(PathBuf),
    Removed(PathBuf),
    Added(PathBuf),
}

impl Changes {
    pub fn detect(original_dir: &Path, modified_dir: &Path) -> Result<Self> {
        read_changes(original_dir, modified_dir)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, ChangeKind> {
        self.changes.iter()
    }

    pub fn original_path(&self, path: &Path) -> PathBuf {
        self.base_dir_original.join(path)
    }
    pub fn modified_path(&self, path: &Path) -> PathBuf {
        self.base_dir_modified.join(path)
    }
}

impl ChangeKind {
    pub fn actual(&self, changes: &Changes) -> (Option<PathBuf>, Option<PathBuf>) {
        match self {
            ChangeKind::Modified(path) => (
                Some(changes.original_path(path)),
                Some(changes.modified_path(path)),
            ),
            ChangeKind::Added(path) => (None, Some(changes.modified_path(path))),
            ChangeKind::Removed(path) => (Some(changes.original_path(path)), None),
        }
    }

    pub fn inner(&self) -> &Path {
        match self {
            ChangeKind::Modified(val) => val,
            ChangeKind::Removed(val) => val,
            ChangeKind::Added(val) => val,
        }
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

fn read_changes(original: &Path, modified: &Path) -> Result<Changes> {
    ensure!(
        original.exists(),
        "{}: no such file or directory",
        original.display()
    );
    ensure!(
        modified.exists(),
        "{}: no such file or directory",
        modified.display()
    );

    match (original.is_dir(), modified.is_dir()) {
        (true, true) => read_changes_dir(original, modified),
        (false, false) => Err(eyre!("Diffing files is not implemented yet")),
        _ => Err(eyre!(
            "Cannot diffpatch mix of path and directory {} and {}",
            original.display(),
            modified.display()
        )),
    }
}

fn read_changes_dir(original_dir: &Path, modified_dir: &Path) -> Result<Changes> {
    let original_paths = read_diff_paths(original_dir)?;
    let modified_paths = read_diff_paths(modified_dir)?;

    let modified = original_paths.intersection(&modified_paths);
    let removed = original_paths.difference(&modified_paths);
    let added = modified_paths.difference(&original_paths);

    let changes = modified
        .map(|p| ChangeKind::Modified(p.to_owned()))
        .chain(removed.map(|p| ChangeKind::Removed(p.to_owned())))
        .chain(added.map(|p| ChangeKind::Added(p.to_owned())))
        .collect();

    Ok(Changes {
        base_dir_original: original_dir.to_owned(),
        base_dir_modified: modified_dir.to_owned(),
        changes,
    })
}
