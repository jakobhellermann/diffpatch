use color_eyre::Result;
use color_eyre::eyre::{Context, bail, eyre};
use std::str::FromStr;

pub enum Interface {
    Direct,
    InlineClear,
}
impl FromStr for Interface {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "direct" => Ok(Interface::Direct),
            "inline-clear" => Ok(Interface::InlineClear),
            other => Err(ParseEnumError(
                &["direct", "inline-clear"],
                other.to_owned(),
            )),
        }
    }
}

pub struct Options {
    // diff options
    pub context_len: usize,
    pub reversed: bool,

    // interface options
    pub interface: Interface,
    pub immediate_command: bool,

    // misc
    pub jj_subcommand: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            context_len: 3,
            reversed: false,

            interface: Interface::Direct,
            immediate_command: true,

            jj_subcommand: None,
        }
    }
}

impl Options {
    pub fn load_env(&mut self) -> Result<&mut Options> {
        get_env(&mut self.context_len, "DIFFPATCH_CONTEXT_LEN")?;

        get_env(&mut self.interface, "DIFFPATCH_INTERFACE")?;
        get_env_bool(&mut self.immediate_command, "DIFFPATCH_IMMEDIATE_COMMAND")?;

        Ok(self)
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

#[derive(Debug)]
pub struct ParseEnumError(&'static [&'static str], String);
impl std::fmt::Display for ParseEnumError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Expected one of ")?;
        for possible in self.0 {
            write!(f, "{possible}, ")?;
        }
        write!(f, "got '{}'", self.1)
    }
}
impl std::error::Error for ParseEnumError {}
