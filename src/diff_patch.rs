use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::Result;
use diffy::{Patch, PatchFormatter};
use nu_ansi_term::{Color, Style};
use termion::cursor::DetectCursorPos;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};

use crate::changes::{ChangeKind, Changes};

pub struct Options {
    // diff options
    context_len: usize,

    // interface options
    clear_after_hunk: bool,
    immediate_command: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            context_len: 3,

            clear_after_hunk: false,
            immediate_command: true,
        }
    }
}

pub struct DiffPatch {
    options: Options,
    formatter: PatchFormatter,

    stdin: std::io::Stdin,
    stdout: RawTerminal<std::io::Stdout>,

    uncleared_lines: (u16, u16),
}

#[derive(Clone, Copy, Default)]
struct Step {
    change: usize,
    hunk: usize,
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
            uncleared_lines: (0, 0),
        })
    }

    pub fn run(&mut self, changes: &Changes) -> Result<()> {
        if changes.changes.is_empty() {
            return Ok(());
        }

        let mut step = Step::default();

        loop {
            let change = &changes.changes[step.change];

            let (original, modified) = change.actual(changes);

            let original_content = original
                .map(std::fs::read_to_string)
                .transpose()?
                .unwrap_or_default();
            let modified_content = modified
                .map(std::fs::read_to_string)
                .transpose()?
                .unwrap_or_default();

            let path = change.inner();
            let mut diff_options = diffy::DiffOptions::new();
            diff_options.set_context_len(self.options.context_len);
            diff_options.set_original_filename(path.display().to_string());
            diff_options.set_modified_filename(path.display().to_string());
            let patch = diff_options.create_patch(&original_content, &modified_content);

            self.step(change, &patch, step)?;

            step.hunk += 1;
            if step.hunk >= patch.hunks().len() {
                step.hunk = 0;
                step.change += 1;
            }
            if step.change >= changes.changes.len() {
                break;
            }
        }

        Ok(())
    }

    fn step(&mut self, change: &ChangeKind, patch: &Patch<'_, str>, step: Step) -> Result<()> {
        let size = termion::terminal_size()?;
        let n_hunks = patch.hunks().len();
        let hunk = patch.hunks().get(step.hunk);

        let mut writer = CountLines::new(self.stdout.lock());

        if step.hunk == 0 {
            assert!(!self.options.clear_after_hunk || self.uncleared_lines.0 == 0);

            let path = change.inner();
            write_header(&mut writer, Some(path), Some(path))?;
            self.uncleared_lines.0 = writer.take_lineno();
        }

        if let Some(hunk) = hunk {
            assert!(!self.options.clear_after_hunk || self.uncleared_lines.1 == 0);
            self.formatter.write_hunk_into(hunk, &mut writer)?;
            self.uncleared_lines.1 = writer.take_lineno();
        }

        self.ask(&format!(
            "({}/{}) Stage this hunk [y,n,q,a,d,e]? ",
            step.hunk + 1,
            n_hunks
        ))?;

        if self.options.clear_after_hunk {
            let last = step.hunk == n_hunks.saturating_sub(1);

            let erase = std::mem::take(&mut self.uncleared_lines.1)
                + match last {
                    true => std::mem::take(&mut self.uncleared_lines.0),
                    false => 0,
                };

            self.stdout.activate_raw_mode()?;
            let new_pos = self.stdout.cursor_pos()?;
            self.stdout.suspend_raw_mode()?;

            let lines = erase + 1;
            let extra = (new_pos.1 == size.1) as u16;
            self.erase_last_lines(lines + extra)?;
        }

        Ok(())
    }

    fn erase_last_lines(&mut self, n: u16) -> Result<(u16, u16)> {
        self.stdout.activate_raw_mode()?;
        let pos = self.stdout.cursor_pos()?;
        self.stdout.suspend_raw_mode()?;

        let new_pos = (pos.0, pos.1.saturating_sub(n));
        write!(
            self.stdout,
            "{}{}",
            termion::cursor::Goto(1, new_pos.1),
            termion::clear::AfterCursor,
        )?;
        self.stdout.flush()?;

        Ok(pos)
    }
}

struct CountLines<W>(W, usize);
impl<W> CountLines<W> {
    fn new(w: W) -> Self {
        CountLines(w, 0)
    }
    fn take_lineno(&mut self) -> u16 {
        std::mem::take(&mut self.1) as u16
    }
}
impl<W: std::io::Write> std::io::Write for CountLines<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.1 += buf.iter().filter(|&&x| x == b'\n').count();
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
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
