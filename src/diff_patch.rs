use std::borrow::Cow;
use std::io::{BufRead, IsTerminal, Write};
use std::iter;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use color_eyre::Result;
use color_eyre::eyre::{Context, ensure, eyre};
use crossterm::cursor::{self, MoveTo};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use diffy::{Hunk, Patch, PatchFormatter};
use nu_ansi_term::{Color, Style};

use crate::changes::{ChangeKind, Changes};
use crate::config::{Interface, Options};
use crate::count_lines::CountLines;

pub struct DiffPatch {
    options: Options,
    formatter: PatchFormatter,
    plain_formatter: PatchFormatter,

    stdin: std::io::Stdin,
    stdout: MaybeRawTerminal<std::io::Stdout>,

    _alternate: Option<AlternateScreen>,

    inline_uncleared_lines: (u16, u16),
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
    pub fn new(mut options: Options) -> Result<Self> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let is_tty = stdout.is_terminal();

        if !is_tty {
            options.immediate_command = false;
            options.interface = Interface::Direct;
        }

        let wants_raw_terminal = options.immediate_command
            || matches!(
                options.interface,
                Interface::InlineClear | Interface::Fullscreen
            );
        let stdout = if wants_raw_terminal {
            let mut term = RawTerminal::new(stdout);
            term.activate_raw_mode()
                .context("Could not set terminal into raw mode")?;
            term.suspend_raw_mode()?;
            MaybeRawTerminal::Raw(term)
        } else {
            MaybeRawTerminal::Normal(stdout)
        };

        let alternate = matches!(options.interface, Interface::Fullscreen)
            .then(|| -> std::io::Result<_> {
                execute!(std::io::stdout(), EnterAlternateScreen, MoveTo(0, 0))?;
                Ok(AlternateScreen)
            })
            .transpose()
            .context("Could not open fullscreen terminal")?;

        Ok(DiffPatch {
            options,
            formatter: PatchFormatter::new().with_color(),
            plain_formatter: PatchFormatter::new(),
            stdin,
            stdout,
            _alternate: alternate,
            inline_uncleared_lines: (0, 0),
        })
    }

    pub fn run(&mut self, changes: &Changes) -> Result<ExitCode> {
        if changes.changes.is_empty() {
            return Ok(ExitCode::SUCCESS);
        }

        let mut resolutions = vec![Vec::<bool>::new(); changes.changes.len()];

        let contents: Vec<(String, String)> = changes
            .iter()
            .map(|change| {
                let (original, modified) = change.actual(changes);

                let original_content = original
                    .map(std::fs::read_to_string)
                    .transpose()
                    .with_context(|| {
                        format!("failed to read original '{}'", change.inner().display())
                    })?
                    .unwrap_or_default();
                let modified_content = modified
                    .map(std::fs::read_to_string)
                    .transpose()
                    .with_context(|| {
                        format!("failed to read modified '{}'", change.inner().display())
                    })?
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

            let patch = &mut patches[step.change];
            let n_hunks = patch.hunks().len();
            let n_hunks_logical = n_hunks.max(1);

            resolutions[step.change].resize(n_hunks_logical, false);

            if step.hunk == STEP_HUNK_LAST {
                step.hunk = n_hunks.saturating_sub(1);
            }

            self.step(change, patch, prev_step, step)?;

            let kind = match (self.options.reversed, change) {
                (_, ChangeKind::Modified(_)) => "this hunk",
                (false, ChangeKind::Removed(_)) | (true, ChangeKind::Added(_)) => "deletion",
                (false, ChangeKind::Added(_)) | (true, ChangeKind::Removed(_)) => "deletion",
            };
            let message = match self.options.jj_subcommand.as_deref().unwrap_or_default() {
                "diffedit" => format!("Keep {kind}"),
                "restore" => format!("Discard {kind}"),
                "commit" | _ => format!("Stage {kind}"),
            };

            let action = self.ask_action(&format!(
                "({}/{}) {} [y,n,q,a,d,e]? ",
                step.hunk + 1,
                n_hunks_logical,
                message
            ))?;

            match action {
                Action::HunkYes => resolutions[step.change][step.hunk] = true,
                Action::HunkNo => resolutions[step.change][step.hunk] = false,
                Action::FileYes => resolutions[step.change][..n_hunks_logical]
                    .iter_mut()
                    .for_each(|x| *x = true),
                Action::FileNo => resolutions[step.change][..n_hunks_logical]
                    .iter_mut()
                    .for_each(|x| *x = false),
                _ => {}
            }

            let mut finish = false;

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
                    finish = true;
                }
                Action::Edit => {
                    if step.hunk < patch.hunks().len() {
                        let original = contents[step.change].0.as_str();
                        if self.edit_hunk(change, patch, step.hunk, original)? {
                            resolutions[step.change][step.hunk] = true;
                            step.hunk += 1;
                        }
                    } else {
                        self.write_error("Sorry, cannot edit this hunk")?;
                    }
                }
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
                Action::Split => {
                    let split_range = patch.split_hunk_at(step.hunk);
                    if split_range.len() == 1 {
                        self.write_error("Sorry, cannot split this hunk")?;
                    } else {
                        let file_resolutions = &mut resolutions[step.change];

                        let resolution = file_resolutions[split_range.start];
                        file_resolutions.splice(
                            split_range.start..split_range.start + 1,
                            iter::repeat_n(resolution, split_range.len()),
                        );
                    }
                }
                Action::Exit => return Ok(ExitCode::FAILURE),
                Action::Clear | Action::None => (),
            }
            if step.hunk != STEP_HUNK_LAST
                && (n_hunks == 0 && step.hunk > 0 || n_hunks > 0 && step.hunk >= n_hunks)
            {
                step.hunk = 0;
                step.change += 1;
            }
            if step.change >= changes.changes.len() {
                finish = true;
            }

            if let Action::Clear = action {
                self.clear_all()?;
            } else {
                let clear_header = prev_step.change != step.change;
                self.clear(clear_header)?;
            }

            if finish {
                break;
            }
        }

        for (((change, patch), (original, _)), file_resolution) in changes
            .iter()
            .zip(&mut patches)
            .zip(&contents)
            .zip(&mut resolutions)
        {
            file_resolution.resize(patch.hunks().len().max(1), false);
            for (hunk, &hunk_resolution) in patch.hunks_mut().iter_mut().zip(&*file_resolution) {
                if hunk_resolution == false {
                    *hunk = Hunk::default();
                }
            }
            apply_change(changes, change, original, patch, file_resolution)?;
        }

        Ok(ExitCode::SUCCESS)
    }

    fn step(
        &mut self,
        change: &ChangeKind,
        patch: &Patch<'_, str>,
        prev_step: Step,
        step: Step,
    ) -> Result<()> {
        let size = self.term_size()?;

        let hunk = patch.hunks().get(step.hunk);

        let mut writer = CountLines::new(self.stdout.lock(), size.0);

        if prev_step.change != step.change {
            let path = change.inner();
            write_header(&mut writer, Some(path), Some(path))?;
            self.inline_uncleared_lines.0 = writer.take_lineno();
        }

        if let Some(hunk) = hunk {
            self.formatter
                .write_hunk_into(&reverse_if(hunk, self.options.reversed), &mut writer)?;
            self.inline_uncleared_lines.1 = writer.take_lineno();
        }

        Ok(())
    }

    fn term_size(&self) -> Result<(u16, u16), std::io::Error> {
        self.stdout
            .is_raw()
            .then(terminal::size)
            .unwrap_or(Ok((u16::MAX, u16::MAX)))
    }

    fn clear(&mut self, clear_header: bool) -> Result<()> {
        match self.options.interface {
            Interface::Direct => {}
            Interface::Fullscreen => {
                if clear_header {
                    self.clear_all()?;
                    self.inline_uncleared_lines.0 = 0;
                } else {
                    let header_lines = self.inline_uncleared_lines.0;
                    write!(
                        self.stdout,
                        "{}{}",
                        MoveTo(0, header_lines),
                        Clear(ClearType::FromCursorDown),
                    )?;
                }
            }
            Interface::InlineClear => {
                let clear_header = clear_header
                    .then(|| std::mem::take(&mut self.inline_uncleared_lines.0))
                    .unwrap_or(0);
                let clear_hunk = std::mem::take(&mut self.inline_uncleared_lines.1);
                self.erase_last_lines(clear_header + clear_hunk + 1)?;
            }
        }

        Ok(())
    }

    fn clear_all(&mut self) -> Result<()> {
        write!(self.stdout, "{}", Clear(ClearType::All))?;
        write!(self.stdout, "{}", MoveTo(0, 0))?;
        Ok(())
    }

    fn erase_last_lines(&mut self, n: u16) -> Result<(u16, u16)> {
        let pos = self.cursor_pos()?;

        let new_pos = (pos.0, pos.1.saturating_sub(n));
        write!(
            self.stdout,
            "{}{}",
            MoveTo(0, new_pos.1),
            Clear(ClearType::FromCursorDown),
        )?;
        self.stdout.flush()?;

        Ok(pos)
    }

    fn write_error(&mut self, msg: &str) -> Result<(), std::io::Error> {
        let error_style = nu_ansi_term::Style::new().fg(Color::Red).bold();
        writeln!(self.stdout, "{}", error_style.paint(msg))
    }

    fn ask_action(&mut self, msg: &str) -> Result<Action> {
        let style = nu_ansi_term::Style::new().fg(Color::Blue).bold();

        let mut stdout = std::io::stdout().lock();
        let mut ask = || {
            write!(self.stdout, "{}", style.paint(msg))?;
            stdout.flush()
        };

        let result = if self.options.immediate_command {
            ask()?;

            let result = self.keys(|key| {
                let action = match key {
                    Key::Char(c) => match Action::from_char(c) {
                        Some(action) => action,
                        None => return Ok(ControlFlow::Continue(())),
                    },
                    Key::Ctrl('c') => Action::Exit,
                    Key::Ctrl('l') => Action::Clear,
                    Key::Left | Key::Up => Action::Prev,
                    Key::Right | Key::Down => Action::Next,
                    _ => return Ok(ControlFlow::Continue(())),
                };
                Ok(ControlFlow::Break(action))
            })?;
            writeln!(self.stdout)?;

            result.unwrap_or(Action::None)
        } else {
            let mut line = String::new();

            loop {
                ask()?;
                line.clear();
                BufRead::read_line(&mut self.stdin.lock(), &mut line)?;

                match Action::from_str(line.trim_end_matches('\n')) {
                    Some(action) => break action,
                    None => continue,
                }
            }
        };

        Ok(result)
    }

    fn cursor_pos(&mut self) -> Result<(u16, u16)> {
        let term = self.stdout.get_raw()?;

        term.activate_raw_mode()?;
        let pos = cursor::position()?;
        term.suspend_raw_mode()?;

        Ok(pos)
    }

    fn keys<B>(&mut self, mut f: impl FnMut(Key) -> Result<ControlFlow<B>>) -> Result<Option<B>> {
        self.stdout.get_raw()?.activate_raw_mode()?;

        let result = loop {
            let ev = event::read()?;
            let Some(key) = Key::from_event(ev) else {
                continue;
            };
            match f(key)? {
                ControlFlow::Continue(()) => continue,
                ControlFlow::Break(b) => break Some(b),
            }
        };

        self.stdout.suspend_raw_mode()?;
        Ok(result)
    }

    /// Open the external editor on a single hunk and validate that the result
    /// still applies to the original before accepting it. If the edit fails to
    /// parse or to apply, the user is shown the reason and asked whether to edit again.
    ///
    /// Returns `true` if the hunk was edited and accepted, `false` if the edit
    /// was discarded or aborted (all lines removed) and the hunk left unchanged.
    fn edit_hunk(
        &mut self,
        change: &ChangeKind,
        patch: &mut Patch<str>,
        idx: usize,
        original: &str,
    ) -> Result<bool> {
        let path = change.inner();
        let mut content = {
            let display_hunk = reverse_if(&patch.hunks()[idx], self.options.reversed);
            self.plain_formatter.fmt_hunk(&display_hunk).to_string()
        };

        loop {
            let edited = self.edit(&content)?;

            // Mirrors git's behaviour: removing every line aborts the edit.
            if edited.trim().is_empty() {
                return Ok(false);
            }

            let new_hunk = match Hunk::from_str(edited.clone().leak(), true) {
                Ok(hunk) => reverse_if(&hunk, self.options.reversed).into_owned(),
                Err(err) => {
                    self.write_error(&format!("error: could not parse edited hunk: {err}"))?;
                    if self.ask_edit_again()? {
                        content = edited;
                        continue;
                    }
                    return Ok(false);
                }
            };

            let start_line = new_hunk.old_range().start();
            let previous = std::mem::replace(&mut patch.hunks_mut()[idx], new_hunk);
            match diffy::apply(original, patch) {
                Ok(_) => return Ok(true),
                Err(_) => {
                    patch.hunks_mut()[idx] = previous;
                    self.write_error(&format!(
                        "error: {}:{}: patch does not apply",
                        path.display(),
                        start_line
                    ))?;
                    if self.ask_edit_again()? {
                        content = edited;
                        continue;
                    }
                    return Ok(false);
                }
            }
        }
    }

    fn ask_edit_again(&mut self) -> Result<bool> {
        self.ask_yes_no(
            "Your edited hunk does not apply. Edit again (saying \"no\" discards!) [y/n]? ",
        )
    }

    fn ask_yes_no(&mut self, msg: &str) -> Result<bool> {
        let style = nu_ansi_term::Style::new().fg(Color::Blue).bold();

        let mut stdout = std::io::stdout().lock();
        let mut ask = || {
            write!(self.stdout, "{}", style.paint(msg))?;
            stdout.flush()
        };

        if self.options.immediate_command {
            ask()?;
            let result = self.keys(|key| {
                Ok(match key {
                    Key::Char('y') | Key::Char('Y') => ControlFlow::Break(true),
                    Key::Char('n') | Key::Char('N') | Key::Ctrl('c') => ControlFlow::Break(false),
                    _ => ControlFlow::Continue(()),
                })
            })?;
            writeln!(self.stdout)?;
            Ok(result.unwrap_or(false))
        } else {
            loop {
                ask()?;
                let mut line = String::new();
                BufRead::read_line(&mut self.stdin.lock(), &mut line)?;
                match line.trim() {
                    "y" | "Y" => return Ok(true),
                    "n" | "N" => return Ok(false),
                    _ => continue,
                }
            }
        }
    }

    fn edit(&self, hunk: &str) -> Result<String> {
        let msg = format!("{EDIT_HUNK_HEADER}\n{hunk}{EDIT_HUNK_TRAILER}");
        let path = hunk_edit_path(&std::env::current_dir()?);
        std::fs::write(&path, msg)?;

        let mut cmd = Command::new("nvim").arg(&path).spawn()?;
        let status = cmd.wait()?;
        ensure!(status.success(), "Error running external editor");

        let edited = std::fs::read_to_string(path)?;
        let without_comments = edited
            .split_inclusive('\n')
            .filter(|line| !line.starts_with('#'))
            .collect::<String>();
        Ok(without_comments)
    }
}

fn hunk_edit_path(cwd: &Path) -> PathBuf {
    let vcs_dir = iter::successors(Some(cwd), |path| path.parent()).find_map(|dir| {
        let jj_dir = dir.join(".jj");
        if jj_dir.is_dir() {
            Some(jj_dir)
        } else {
            let git_dir = dir.join(".git");
            git_dir.is_dir().then_some(git_dir)
        }
    });
    let dir = vcs_dir.unwrap_or_else(std::env::temp_dir);

    dir.join("addp-hunk-edit.diff")
}

const EDIT_HUNK_HEADER: &str = "# Manual hunk edit mode -- see bottom for a quick guide.";
const EDIT_HUNK_TRAILER: &str = "# ---
# To remove '-' lines, make them ' ' lines (context).
# To remove '+' lines, delete them.
# Lines starting with # will be removed.
# If the patch applies cleanly, the edited hunk will immediately be marked for staging.
# If it does not apply cleanly, you will be given an opportunity to
# edit again.  If all lines of the hunk are removed, then the edit is
# aborted and the hunk is left unchanged.
";

enum Action {
    HunkYes,
    HunkNo,
    FileYes,
    FileNo,
    Split,
    Edit,
    Quit,
    Prev,
    Next,

    Clear,
    Exit,
    None,
}

impl Action {
    fn from_char(c: char) -> Option<Action> {
        Some(match c {
            'y' => Action::HunkYes,
            'n' => Action::HunkNo,
            'a' => Action::FileYes,
            'd' => Action::FileNo,
            's' => Action::Split,
            'e' => Action::Edit,
            'q' => Action::Quit,
            'l' => Action::Clear,
            _ => return None,
        })
    }

    fn from_str(s: &str) -> Option<Action> {
        match s {
            "\x1b[D" | "\x1b[A" => Some(Action::Prev),
            "\x1b[C" | "\x1b[B" => Some(Action::Next),
            other => {
                let mut chars = other.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                Action::from_char(c)
            }
        }
    }
}

fn write_header(
    mut w: impl Write,
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
    file_resolution: &[bool],
) -> Result<()> {
    let applied = diffy::apply(original, patch)
        .with_context(|| format!("Could not apply patch\n{}", patch.to_string()))?;

    let original_path = changes.original_path(change.inner());
    let modified_path = changes.modified_path(change.inner());
    match change {
        ChangeKind::Modified(_) => {
            std::fs::write(&modified_path, applied).context("error applying file modification")?
        }
        ChangeKind::Removed(_) => {
            assert_eq!(file_resolution.len(), 1);
            let resolution = file_resolution[0];

            if resolution == false {
                std::fs::copy(&original_path, &modified_path)
                    .context("error applying file removal")?;
            }
        }
        ChangeKind::Added(_) => {
            assert_eq!(file_resolution.len(), 1);

            let resolution = file_resolution[0];
            if resolution == false {
                std::fs::remove_file(modified_path).context("error applying file addition")?;
            }
        }
    }

    Ok(())
}

fn reverse_if<'h, 'c>(hunk: &'c Hunk<'h, str>, reverse: bool) -> Cow<'c, Hunk<'h, str>> {
    match reverse {
        true => Cow::Owned(hunk.reverse()),
        false => Cow::Borrowed(hunk),
    }
}

/// Guards crossterm's global raw mode: restores cooked mode on drop, like termion's `RawTerminal`.
struct RawTerminal<W> {
    inner: W,
    raw_active: bool,
}
impl<W> RawTerminal<W> {
    fn new(inner: W) -> Self {
        RawTerminal {
            inner,
            raw_active: false,
        }
    }

    fn inner_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    fn activate_raw_mode(&mut self) -> std::io::Result<()> {
        if !self.raw_active {
            terminal::enable_raw_mode()?;
            self.raw_active = true;
        }
        Ok(())
    }
    fn suspend_raw_mode(&mut self) -> std::io::Result<()> {
        if self.raw_active {
            terminal::disable_raw_mode()?;
            self.raw_active = false;
        }
        Ok(())
    }
}
impl<W> Drop for RawTerminal<W> {
    fn drop(&mut self) {
        let _ = self.suspend_raw_mode();
    }
}

/// Leaves the alternate screen on drop, like termion's `AlternateScreen` guard.
struct AlternateScreen;
impl Drop for AlternateScreen {
    fn drop(&mut self) {
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
    }
}

/// The subset of keys this module handles, mapped from crossterm events.
enum Key {
    Char(char),
    Ctrl(char),
    Left,
    Right,
    Up,
    Down,
}
impl Key {
    fn from_event(ev: Event) -> Option<Self> {
        let Event::Key(key) = ev else {
            return None;
        };
        if key.kind == KeyEventKind::Release {
            return None;
        }
        match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    Some(Key::Ctrl(c.to_ascii_lowercase()))
                } else if key
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::META | KeyModifiers::SUPER)
                {
                    None
                } else {
                    Some(Key::Char(c))
                }
            }
            KeyCode::Left => Some(Key::Left),
            KeyCode::Right => Some(Key::Right),
            KeyCode::Up => Some(Key::Up),
            KeyCode::Down => Some(Key::Down),
            _ => None,
        }
    }
}

enum MaybeRawTerminal<W: Write> {
    Raw(RawTerminal<W>),
    Normal(W),
}
impl<W: Write> MaybeRawTerminal<W> {
    fn inner_mut(&mut self) -> &mut W {
        match self {
            MaybeRawTerminal::Raw(term) => term.inner_mut(),
            MaybeRawTerminal::Normal(w) => w,
        }
    }

    #[track_caller]
    fn get_raw(&mut self) -> Result<&mut RawTerminal<W>> {
        match self {
            MaybeRawTerminal::Raw(term) => Ok(term),
            MaybeRawTerminal::Normal(_) => {
                Err(eyre!("Attempted to get raw terminal, but it isn't enabled"))
            }
        }
    }
    fn is_raw(&self) -> bool {
        matches!(self, MaybeRawTerminal::Raw(_))
    }
    fn suspend_raw_mode(&mut self) -> Result<(), std::io::Error> {
        match self {
            MaybeRawTerminal::Raw(term) => term.suspend_raw_mode(),
            MaybeRawTerminal::Normal(_) => Ok(()),
        }
    }
}
impl<W: Write> Write for MaybeRawTerminal<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner_mut().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner_mut().flush()
    }
}
impl MaybeRawTerminal<std::io::Stdout> {
    fn lock(&mut self) -> std::io::StdoutLock<'static> {
        self.inner_mut().lock()
    }
}
