use std::process::Command;

use anyhow::{Context, Result};

pub fn resolve_token(configured_token: Option<&str>) -> Result<String> {
    if let Some(token) = configured_token.filter(|token| !token.trim().is_empty()) {
        return Ok(token.to_string());
    }

    let output = Command::new("gh")
        .args(["auth", "token"])
        .output()
        .context("failed to run `gh auth token`")?;

    if !output.status.success() {
        anyhow::bail!(
            "`gh auth token` failed; set github.token in config or login with `gh auth login`"
        );
    }

    let token =
        String::from_utf8(output.stdout).context("`gh auth token` returned invalid utf-8")?;
    let token = token.trim();
    if token.is_empty() {
        anyhow::bail!(
            "`gh auth token` returned an empty token; set github.token in config or login with `gh auth login`"
        );
    }

    Ok(token.to_string())
}
