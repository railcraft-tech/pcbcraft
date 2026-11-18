use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

pub struct KiCadCli {
    pub bin: PathBuf,
}

impl KiCadCli {
    pub fn from_env() -> Self {
        let bin = std::env::var("KICAD_CLI").unwrap_or_else(|_| "kicad-cli".into());
        Self {
            bin: PathBuf::from(bin),
        }
    }

    pub fn run(&self, args: &[String]) -> Result<String> {
        let output = Command::new(&self.bin)
            .args(args)
            .output()
            .with_context(|| format!("cannot run {}", self.bin.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "{} {} failed:\n{stderr}",
                self.bin.display(),
                args.join(" ")
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub fn run_to_file(&self, args: &[String]) -> Result<()> {
        self.run(args).map(|_| ())
    }
}

pub fn arg(flag: &str, value: impl AsRef<Path>) -> [String; 2] {
    [flag.to_string(), value.as_ref().display().to_string()]
}
