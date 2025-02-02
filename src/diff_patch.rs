use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::Result;
use diffy::PatchFormatter;
use nu_ansi_term::{Color, Style};

use crate::changes::Changes;

#[derive(Default)]
pub struct Options {}

pub struct DiffPatch {
    options: Options,
    formatter: PatchFormatter,
}

impl DiffPatch {
    pub fn new(options: Options) -> Self {
        DiffPatch {
            options,
            formatter: PatchFormatter::new().with_color(),
        }
    }

    pub fn run(&self, changes: &Changes) -> Result<()> {
        for path in &changes.modified {
            let original_contents = changes.original_contents(&path)?;
            let modified_contents = changes.modified_contents(&path)?;

            self.step(&path, &original_contents, &path, &modified_contents)?;
        }

        Ok(())
    }

    pub fn step(
        &self,
        original_path: &Path,
        original_content: &str,
        modified_path: &Path,
        modified_content: &str,
    ) -> Result<()> {
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
            println!("{}", self.formatter.fmt_hunk(hunk));

            ask(&format!(
                "({}/{}) Stage this hunk [y,n,q,a,d,e]? ",
                i + 1,
                n_hunks
            ))?;
        }

        Ok(())
    }
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
