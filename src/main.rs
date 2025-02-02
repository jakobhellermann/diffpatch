use anyhow::{Context, Result, ensure};
use diffy::PatchFormatter;
use nu_ansi_term::{Color, Style};
use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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

struct DiffPatch {}

#[derive(Default)]
struct Options {}

fn main() -> Result<()> {
    let options = Options::default();

    let mut args = std::env::args().skip(1);
    let left = PathBuf::from(args.next().context("missing left path")?);
    let right = PathBuf::from(args.next().context("missing right path")?);

    let left_paths = read_diff_paths(&left)?;
    let right_paths = read_diff_paths(&right)?;

    let modified: Vec<_> = left_paths.intersection(&right_paths).collect();
    let _removed: Vec<_> = left_paths.difference(&right_paths).collect();
    let _added: Vec<_> = right_paths.difference(&left_paths).collect();

    let formatter = PatchFormatter::new().with_color();

    for modified in modified {
        let left_contents = std::fs::read_to_string(left.join(modified))?;
        let right_contents = std::fs::read_to_string(right.join(modified))?;

        step(
            &options,
            &formatter,
            &modified,
            &left_contents,
            &modified,
            &right_contents,
        )?;
    }

    Ok(())
}

fn step(
    _options: &Options,
    formatter: &PatchFormatter,
    original_path: &Path,
    original_content: &str,
    modified_path: &Path,
    modified_content: &str,
) -> Result<(), std::io::Error> {
    let mut options = diffy::DiffOptions::new();

    let original_path_str = original_path.display().to_string();
    let modified_path_string = modified_path.display().to_string();
    options.set_original_filename(original_path_str);
    options.set_modified_filename(modified_path_string);

    let patch = options.create_patch(&original_content, &modified_content);

    let n_hunks = patch.hunks().len();

    write_header(
        std::io::stdout().lock(),
        Some(&original_path),
        Some(&modified_path),
    )?;

    for (i, hunk) in patch.hunks().iter().enumerate() {
        println!("{}", formatter.fmt_hunk(hunk));

        ask(&format!(
            "({}/{}) Stage this hunk [y,n,q,a,d,e]? ",
            i + 1,
            n_hunks
        ))?;
    }

    Ok(())
}

fn write_header(
    mut w: impl std::io::Write,
    filename_original: Option<&Path>,
    filename_modified: Option<&Path>,
) -> std::io::Result<()> {
    let has_color = true;
    let style = Style::new().fg(Color::White).bold();

    if has_color {
        write!(w, "{}", style.prefix())?;
    }
    if let Some(original) = filename_original {
        writeln!(w, "--- {}", original.display())?;
    }
    if let Some(modified) = filename_modified {
        writeln!(w, "+++ {}", modified.display())?;
    }
    if has_color {
        write!(w, "{}", style.suffix())?;
    }

    Ok(())
}

fn ask(msg: &str) -> Result<(), std::io::Error> {
    let style = nu_ansi_term::Style::new().fg(Color::Blue).bold();

    let mut stdout = std::io::stdout().lock();
    print!("{}", style.paint(msg));
    stdout.flush()?;

    let mut line = String::new();
    let stdin = std::io::stdin();
    stdin.lock().read_line(&mut line)?;

    Ok(())
}
