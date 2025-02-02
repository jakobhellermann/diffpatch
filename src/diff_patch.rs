use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::Result;
use diffy::PatchFormatter;
use nu_ansi_term::{Color, Style};
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};

use crate::changes::Changes;

pub struct Options {
    immediate_command: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            immediate_command: true,
        }
    }
}

pub struct DiffPatch {
    options: Options,
    formatter: PatchFormatter,

    stdin: std::io::Stdin,
    stdout: RawTerminal<std::io::Stdout>,
}

impl DiffPatch {
    pub fn new(options: Options) -> Result<Self> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout().into_raw_mode()?;
        stdout.suspend_raw_mode()?;
        Ok(DiffPatch {
            options,
            formatter: PatchFormatter::new().with_color(),
            stdin,
            stdout,
        })
    }

    pub fn run(&mut self, changes: &Changes) -> Result<()> {
        for path in &changes.modified {
            let original_contents = changes.original_contents(&path)?;
            let modified_contents = changes.modified_contents(&path)?;

            self.step(&path, &original_contents, &path, &modified_contents)?;
        }

        Ok(())
    }

    pub fn step(
        &mut self,
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
            self.formatter
                .write_hunk_into(hunk, &mut self.stdout.lock())?;

            self.ask(&format!(
                "({}/{}) Stage this hunk [y,n,q,a,d,e]? ",
                i + 1,
                n_hunks
            ))?;
        }

        Ok(())
    }
}

impl DiffPatch {
    fn ask(&mut self, msg: &str) -> Result<(), std::io::Error> {
        let style = nu_ansi_term::Style::new().fg(Color::Blue).bold();

        let mut stdout = std::io::stdout().lock();
        write!(self.stdout, "{}", style.paint(msg))?;
        stdout.flush()?;

        if self.options.immediate_command {
            self.stdout.activate_raw_mode()?;

            for key in self.stdin.lock().keys() {
                let key = key?;

                match key {
                    Key::Ctrl('c') => std::process::exit(1),
                    Key::Char('y' | 'n' | 'q' | 'a' | 'd' | 'e') => break,
                    Key::Char(_) => {}
                    _ => {}
                }
            }

            self.stdout.suspend_raw_mode()?;
            writeln!(self.stdout)?;
        } else {
            let mut line = String::new();
            BufRead::read_line(&mut self.stdin.lock(), &mut line)?;
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
