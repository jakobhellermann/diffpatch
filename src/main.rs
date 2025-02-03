use changes::Changes;
use color_eyre::eyre::{ContextCompat, Result, ensure};
use config::Options;
use std::path::PathBuf;
use std::process::ExitCode;

mod changes;
mod config;
mod count_lines;
mod diff_patch;

use diff_patch::DiffPatch;

fn main() -> Result<ExitCode> {
    color_eyre::install()?;

    let mut args = std::env::args().skip(1);
    let original_dir = PathBuf::from(args.next().context("missing left path")?);
    let modified_dir = PathBuf::from(args.next().context("missing right path")?);
    ensure!(args.count() == 0, "more args than expected");

    let mut options = Options::default();
    options.jj_subcommand = get_jj_subcommand().unwrap_or(None);
    options.reversed = options.jj_subcommand.as_deref() == Some("restore");
    options.load_env()?;
    let mut diff_patch = DiffPatch::new(options)?;

    let changes = Changes::detect(&original_dir, &modified_dir)?;
    diff_patch.run(&changes)
}

// TODO: get this from JJ itself
fn get_jj_subcommand() -> Result<Option<String>> {
    // SAFETY: no preconditions
    let parent = unsafe { libc::getppid() };
    let cmdline = std::fs::read(format!("/proc/{}/cmdline", parent))?;
    let mut split = cmdline.split(|&x| x == b'\0');
    let Some(b"jj") = split.next() else {
        return Ok(None);
    };
    let subcommand = split
        .skip_while(|arg| arg.starts_with(b"-"))
        .next()
        .map(std::str::from_utf8)
        .transpose()?;

    Ok(subcommand.map(|cmd| {
        let prefixes = [("ci", "commit"), ("sq", "squash")];
        prefixes
            .iter()
            .find_map(|&(prefix, subcommand)| cmd.starts_with(prefix).then_some(subcommand))
            .unwrap_or(cmd)
            .to_string()
    }))
}
