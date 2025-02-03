use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::{Context, Result};
use diffy::{Hunk, Patch, PatchFormatter};
use nu_ansi_term::{Color, Style};
use termion::cursor::DetectCursorPos;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};

use crate::changes::{ChangeKind, Changes};
use crate::count_lines::CountLines;
use crate::vec_map::VecMap;

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

const STEP_HUNK_LAST: usize = usize::MAX;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Step {
    change: usize,
    hunk: usize,
}
impl Step {
    fn invalid() -> Self {
        Step {
            change: usize::MAX,
            hunk: usize::MAX,
        }
    }
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

        let mut resolutions = VecMap::<VecMap<bool>>::with_capacity(changes.changes.len());

        let contents: Vec<(String, String)> = changes
            .iter()
            .map(|change| {
                let (original, modified) = change.actual(changes);

                let original_content = original
                    .map(std::fs::read_to_string)
                    .transpose()?
                    .unwrap_or_default();
                let modified_content = modified
                    .map(std::fs::read_to_string)
                    .transpose()?
                    .unwrap_or_default();

                Ok((original_content, modified_content))
            })
            .collect::<Result<_>>()?;

        let mut patches: Vec<Patch<str>> = changes
            .iter()
            .zip(&contents)
            .map(|(change, (original, modified))| {
                let mut diff_options = diffy::DiffOptions::new();
                let path = change.inner();
                diff_options.set_context_len(self.options.context_len);
                diff_options.set_original_filename(path.display().to_string());
                diff_options.set_modified_filename(path.display().to_string());
                diff_options.create_patch(original, modified)
            })
            .collect();

        let mut step = Step::default();
        let mut prev_step = Step::invalid();

        loop {
            let change = &changes.changes[step.change];

            let patch = &patches[step.change];
            let n_hunks = patch.hunks().len();
            let n_hunks_logical = n_hunks.max(1);

            if step.hunk == STEP_HUNK_LAST {
                step.hunk = n_hunks.saturating_sub(1);
            }

            self.step(change, patch, prev_step, step)?;

            let action = self.ask_action(&format!(
                "({}/{}) Stage {} [y,n,q,a,d,e]? ",
                step.hunk + 1,
                n_hunks_logical,
                match change {
                    ChangeKind::Modified(_) => "this hunk",
                    ChangeKind::Removed(_) => "deletion",
                    ChangeKind::Added(_) => "addition",
                },
            ))?;

            match action {
                Action::HunkYes => *resolutions.get_mut(step.change).get_mut(step.hunk) = true,
                Action::HunkNo => *resolutions.get_mut(step.change).get_mut(step.hunk) = false,
                Action::FileYes => resolutions
                    .get_mut(step.change)
                    .set_all(..n_hunks_logical, true),
                Action::FileNo => resolutions
                    .get_mut(step.change)
                    .set_all(..n_hunks_logical, false),
                _ => {}
            }

            let mut exit = false;

            prev_step = step;
            match action {
                Action::HunkYes => step.hunk += 1,
                Action::HunkNo => step.hunk += 1,
                Action::FileYes | Action::FileNo => {
                    step.change += 1;
                    step.hunk = 0;
                }
                Action::Quit => {
                    step = Step::invalid();
                    exit = true;
                }
                Action::Edit => (),
                Action::Next => {
                    let last = step.change == changes.changes.len() - 1
                        && step.hunk == n_hunks.saturating_sub(1);
                    if !last {
                        step.hunk += 1;
                    }
                }
                Action::Prev => {
                    if step.hunk > 0 {
                        step.hunk -= 1;
                    } else if step.change > 0 {
                        step.change = step.change.saturating_sub(1);
                        step.hunk = usize::MAX;
                    }
                }
                Action::None => (),
            }
            if step.hunk != STEP_HUNK_LAST
                && (n_hunks == 0 && step.hunk > 0 || n_hunks > 0 && step.hunk >= n_hunks)
            {
                step.hunk = 0;
                step.change += 1;
            }
            if step.change >= changes.changes.len() {
                exit = true;
            }

            let clear_header = prev_step.change != step.change;
            self.clear(clear_header)?;

            if exit {
                break;
            }
        }

        for (((change, patch), (original, _)), file_resolution) in changes
            .iter()
            .zip(&mut patches)
            .zip(&contents)
            .zip(&resolutions)
        {
            for (hunk, &hunk_resolution) in patch.hunks_mut().iter_mut().zip(file_resolution) {
                if hunk_resolution == false {
                    *hunk = Hunk::default();
                }
            }
            apply_change(changes, change, original, patch, file_resolution)?;
        }

        Ok(())
    }

    fn step(
        &mut self,
        change: &ChangeKind,
        patch: &Patch<'_, str>,
        prev_step: Step,
        step: Step,
    ) -> Result<()> {
        let size = termion::terminal_size()?;
        let hunk = patch.hunks().get(step.hunk);

        let mut writer = CountLines::new(self.stdout.lock(), size.0);

        if prev_step.change != step.change {
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

        Ok(())
    }

    fn clear(&mut self, clear_header: bool) -> Result<()> {
        if self.options.clear_after_hunk {
            let erase = std::mem::take(&mut self.uncleared_lines.1)
                + 1 // ask
                + match clear_header {
                    true => std::mem::take(&mut self.uncleared_lines.0),
                    false => 0,
                };

            self.erase_last_lines(erase)?;
        }

        Ok(())
    }

    fn erase_last_lines(&mut self, n: u16) -> Result<(u16, u16)> {
        let pos = self.cursor_pos()?;

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

    fn cursor_pos(&mut self) -> Result<(u16, u16)> {
        self.stdout.activate_raw_mode()?;
        let new_pos = self.stdout.cursor_pos()?;
        self.stdout.suspend_raw_mode()?;

        Ok(new_pos)
    }
}

enum Action {
    HunkYes,
    HunkNo,
    FileYes,
    FileNo,
    Edit,
    Quit,
    Prev,
    Next,
    None,
}

impl DiffPatch {
    fn ask_action(&mut self, msg: &str) -> Result<Action, std::io::Error> {
        let style = nu_ansi_term::Style::new().fg(Color::Blue).bold();

        let mut stdout = std::io::stdout().lock();
        write!(self.stdout, "{}", style.paint(msg))?;
        stdout.flush()?;

        let result = if self.options.immediate_command {
            self.stdout.activate_raw_mode()?;

            let mut result = Action::None;
            for key in self.stdin.lock().keys() {
                result = match key? {
                    Key::Ctrl('c') => std::process::exit(1),
                    Key::Char('y') => Action::HunkYes,
                    Key::Char('n') => Action::HunkNo,
                    Key::Char('a') => Action::FileYes,
                    Key::Char('d') => Action::FileNo,
                    Key::Char('e') => Action::Edit,
                    Key::Char('q') => Action::Quit,
                    Key::Left | Key::Up => Action::Prev,
                    Key::Right | Key::Down => Action::Next,
                    _ => continue,
                };
                break;
            }

            self.stdout.suspend_raw_mode()?;
            writeln!(self.stdout)?;

            result
        } else {
            let mut line = String::new();
            BufRead::read_line(&mut self.stdin.lock(), &mut line)?;
            todo!()
        };

        Ok(result)
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

fn apply_change(
    changes: &Changes,
    change: &ChangeKind,
    original: &str,
    patch: &Patch<str>,
    file_resolution: &VecMap<bool>,
) -> Result<()> {
    let applied = diffy::apply(original, patch)?;

    let original_path = changes.original_path(change.inner());
    let modified_path = changes.modified_path(change.inner());
    match change {
        ChangeKind::Modified(_) => {
            std::fs::write(&modified_path, applied).context("error applying file modification")?
        }
        ChangeKind::Removed(_) => {
            assert_eq!(file_resolution.len(), 1);
            let resolution = *file_resolution.get(0);

            if resolution == false {
                std::fs::copy(&original_path, &modified_path)
                    .context("error applying file removal")?;
            }
        }
        ChangeKind::Added(_) => {
            assert_eq!(file_resolution.len(), 1);

            let resolution = *file_resolution.get(0);
            if resolution == false {
                std::fs::remove_file(modified_path).context("error applying file addition")?;
            }
        }
    }

    Ok(())
}
