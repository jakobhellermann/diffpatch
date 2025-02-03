use changes::Changes;
use color_eyre::eyre::{ContextCompat, Result, ensure};
use std::path::PathBuf;

mod changes;
mod count_lines;
mod diff_patch;
mod vec_map;

use diff_patch::{DiffPatch, Options};

fn main() -> Result<()> {
    color_eyre::install()?;

    let mut args = std::env::args().skip(1);
    let original_dir = PathBuf::from(args.next().context("missing left path")?);
    let modified_dir = PathBuf::from(args.next().context("missing right path")?);
    ensure!(args.count() == 0, "more args than expected");

    let options = Options::default();
    let mut diff_patch = DiffPatch::new(options)?;

    let changes = Changes::detect(&original_dir, &modified_dir)?;
    diff_patch.run(&changes)?;

    Ok(())
}
