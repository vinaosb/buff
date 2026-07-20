//! `buff login [<TOKEN>]` — authenticate with the buff registry.
//!
//! For the v1.6 milestone, "authenticate" means "store a bearer token
//! the user already has". The registry itself ships static-token
//! provisioning via [`buff_registry::InMemoryStorage::add_token`]; a
//! real OAuth flow (GitHub) is deferred (see the buff-registry crate
//! root docs).
//!
//! # Behavior
//!
//! - If `<TOKEN>` is provided on the command line, it is stored.
//! - If omitted, the CLI reads one line from stdin. This mirrors the
//!   `cargo login` UX (cargo reads from stdin when no arg is given).
//! - The token is written to [`crate::commands::registry::credentials_path`]
//!   (`~/.buff/credentials`) as TOML: `token = "<value>"`. The file is
//!   created with best-effort, panic-free I/O (parent dirs made on
//!   demand; errors surface via [`anyhow::Error`]).
//!
//! # Errors
//!
//! - Fails if home directory can't be resolved (`BUFF_HOME` /
//!   `USERPROFILE` / `HOME` all unset).
//! - Fails if the credentials file can't be written (permissions,
//!   disk full).
//! - Fails if `<TOKEN>` is empty/whitespace.
//!
//! # Future
//!
//! The current registry does NOT expose a "verify token" endpoint, so
//! `buff login` writes the token without round-tripping it. A future
//! `GET /api/v1/whoami` (or similar) on the registry would let us
//! reject bad tokens at login time. Deferred.

use anyhow::{bail, Context, Result};

use crate::commands::registry::{registry_url, save_credentials_to, Credentials};

/// Entry point for `buff login [<TOKEN>]`.
///
/// Reads the token from the positional arg OR stdin (when `token_arg`
/// is `None`), then writes it to the credentials file.
pub fn run(token_arg: Option<&str>) -> Result<()> {
    let token = match token_arg {
        Some(t) => t.trim().to_string(),
        None => read_token_from_stdin()?,
    };
    if token.is_empty() {
        bail!(
            "empty token — pass a non-empty token as `buff login <TOKEN>` \
             or pipe one via stdin"
        );
    }
    let base_url = registry_url();
    run_with_token(&token, &base_url)?;
    eprintln!(
        "Login succeeded — token stored at {}",
        crate::commands::registry::credentials_path()?.display()
    );
    eprintln!("Registry: {base_url}");
    Ok(())
}

/// Same as [`run`] but takes the token + registry URL explicitly
/// (used by integration tests).
pub fn run_with_token(token: &str, base_url: &str) -> Result<()> {
    if token.trim().is_empty() {
        bail!("empty token");
    }
    let creds = Credentials {
        token: Some(token.trim().to_string()),
    };
    save_credentials_to(&creds, &crate::commands::registry::credentials_path()?)?;
    eprintln!("Storing credentials for registry {base_url}");
    Ok(())
}

/// Read one line from stdin, trimmed. Used when no token arg is given.
fn read_token_from_stdin() -> Result<String> {
    eprintln!("Paste your registry token and press Enter:");
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .context("failed to read token from stdin")?;
    Ok(line.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_with_empty_token_errors() {
        let result = run_with_token("", "http://example");
        assert!(result.is_err());
    }

    #[test]
    fn run_with_whitespace_only_token_errors() {
        let result = run_with_token("   ", "http://example");
        assert!(result.is_err());
    }
}
