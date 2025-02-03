use color_eyre::Result;
use color_eyre::eyre::{Context, bail, eyre};
use std::str::FromStr;

pub struct Options {
    // diff options
    pub context_len: usize,

    // interface options
    pub clear_after_hunk: bool,
    pub immediate_command: bool,
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

impl Options {
    pub fn from_env() -> Result<Options> {
        let mut options = Options::default();

        get_env(&mut options.context_len, "DIFFPATCH_CONTEXT_LEN")?;
        get_env_bool(&mut options.clear_after_hunk, "DIFFPATCH_CLEAR_AFTER_HUNK")?;
        get_env_bool(
            &mut options.immediate_command,
            "DIFFPATCH_IMMEDIATE_COMMAND",
        )?;

        Ok(options)
    }
}

fn get_env<T: FromStr>(out: &mut T, env_name: &str) -> Result<()>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    if let Ok(var) = std::env::var(env_name) {
        *out = var
            .parse()
            .with_context(|| eyre!("{}={} could not be parsed", env_name, var))?;
    }
    Ok(())
}
fn get_env_bool(out: &mut bool, env_name: &str) -> Result<()> {
    if let Ok(var) = std::env::var(env_name) {
        *out = match var.as_str() {
            "yes" | "y" | "1" | "true" => true,
            "no" | "n" | "0" | "false" => false,
            _ => bail!("{}={} could not be parsed as a boolean", env_name, var),
        }
    }
    Ok(())
}
