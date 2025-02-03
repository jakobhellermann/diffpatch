use changes::Changes;
use color_eyre::eyre::{ContextCompat, Result, ensure};
use config::Options;
use std::path::PathBuf;

mod changes;
mod config;
mod count_lines;
mod diff_patch;

use diff_patch::DiffPatch;

fn main() -> Result<()> {
    color_eyre::install()?;

    let mut args = std::env::args().skip(1);
    let original_dir = PathBuf::from(args.next().context("missing left path")?);
    let modified_dir = PathBuf::from(args.next().context("missing right path")?);
    ensure!(args.count() == 0, "more args than expected");

    let options = Options::from_env()?;
    let mut diff_patch = DiffPatch::new(options)?;

    let changes = Changes::detect(&original_dir, &modified_dir)?;
    diff_patch.run(&changes)?;

    Ok(())
}
