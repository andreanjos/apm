use anyhow::{bail, Result};
use clap::CommandFactory;
use clap_complete::{generate, Shell};

use crate::Cli;

/// Generate shell completion script for the given shell name and print to stdout.
pub fn run(shell_name: &str) -> Result<()> {
    let shell: Shell = match shell_name.to_lowercase().as_str() {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "elvish" => Shell::Elvish,
        "powershell" | "ps" => Shell::PowerShell,
        other => bail!(
            "Unknown shell '{other}'. Supported shells: bash, zsh, fish, elvish, powershell."
        ),
    };

    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_owned();
    generate(shell, &mut cmd, bin_name, &mut std::io::stdout());

    Ok(())
}
