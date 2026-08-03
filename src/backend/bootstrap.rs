//! First-run bootstrap of the external ESP backend generators.
//!
//! The two ESP stacks shell out to upstream binaries (`esp-generate`,
//! `cargo-generate`) that have no stable library API, so they must be installed
//! as tools. `cargo install` has no post-install hook, so "one command installs
//! everything" cannot live in the crate's install step — it lives here instead:
//! when a generation actually needs a backend that isn't on `PATH`,
//! xiao-generate offers to install the pinned version itself.
//!
//! Two rules shape this:
//!
//! * **Probe at point of use, every run.** We never persist an "installed" flag
//!   — a user can `cargo uninstall` a tool, sync dotfiles to another machine, or
//!   change `PATH` between runs, so any cached bit goes stale. Each generation
//!   re-checks the tool it is about to invoke, exactly like `git`/`rustup` do
//!   with their own external dependencies.
//! * **Never install without consent.** The default is an interactive `[Y/n]`
//!   prompt; `--yes` installs unattended, `--no-install` never installs and just
//!   prints the command. With no terminal to ask on, we decline rather than run
//!   an unattended network install.

use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::registry::ESP_GENERATE_VERSION;

/// How the user wants missing backends handled, derived from CLI flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consent {
    /// Ask on the terminal before installing (the default).
    Prompt,
    /// `--yes`: install a missing backend without asking.
    AssumeYes,
    /// `--no-install`: never install; report the manual command and stop.
    Never,
}

impl Consent {
    /// Map the two CLI flags to a policy. `--no-install` wins if both are set
    /// (clap already makes them mutually exclusive, so that is belt-and-braces).
    pub fn from_flags(yes: bool, no_install: bool) -> Self {
        match (no_install, yes) {
            (true, _) => Consent::Never,
            (false, true) => Consent::AssumeYes,
            (false, false) => Consent::Prompt,
        }
    }
}

/// Ensure `esp-generate` (the pinned version) is available for the `no_std` stack.
pub fn ensure_esp_generate(consent: Consent) -> Result<()> {
    ensure(
        "esp-generate",
        esp_generate_present,
        &["install", "esp-generate", "--version", ESP_GENERATE_VERSION, "--locked"],
        consent,
    )
}

/// Ensure `cargo-generate` is available for the ESP `std` (esp-idf) stack.
pub fn ensure_cargo_generate(consent: Consent) -> Result<()> {
    ensure("cargo-generate", cargo_generate_present, &["install", "cargo-generate"], consent)
}

fn esp_generate_present() -> bool {
    runs(Command::new("esp-generate").arg("--version"))
}

fn cargo_generate_present() -> bool {
    runs(Command::new("cargo").args(["generate", "--version"]))
}

fn runs(cmd: &mut Command) -> bool {
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

/// The shared check → consent → install → re-check flow. `present` is probed
/// twice: once to decide whether anything is needed, and again after a install
/// to confirm the binary actually landed on `PATH`.
fn ensure(label: &str, present: fn() -> bool, install_args: &[&str], consent: Consent) -> Result<()> {
    if present() {
        return Ok(());
    }

    let manual = format!("cargo {}", install_args.join(" "));
    match consent {
        Consent::Never => bail!(
            "{label} is required for this stack but is not installed, and --no-install was given.\n    \
             install it yourself with:\n        {manual}"
        ),
        Consent::AssumeYes => {}
        Consent::Prompt => {
            let q = format!(
                "{label} is required for this stack but isn't installed.\n  install it now with `{manual}`?"
            );
            if !confirm(&q) {
                bail!(
                    "{label} is required for this stack. install it with:\n        {manual}\n    \
                     (or re-run with --yes to install it automatically)"
                );
            }
        }
    }

    eprintln!("→ installing {label}: {manual}");
    let status = Command::new("cargo")
        .args(install_args)
        .status()
        .with_context(|| format!("failed to launch `{manual}`"))?;
    if !status.success() {
        bail!("`{manual}` failed — install {label} manually and re-run");
    }
    if !present() {
        bail!(
            "{label} was installed but is still not runnable — make sure ~/.cargo/bin is on your PATH, then re-run"
        );
    }
    Ok(())
}

/// Ask a yes/no question on the terminal. Enter (empty) means yes, because the
/// user is actively running a command that needs the tool. Returns `false`
/// whenever there is no terminal to ask on, so a non-interactive run never
/// triggers an unattended install.
fn confirm(question: &str) -> bool {
    use std::io::{BufRead, IsTerminal, Write};

    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("{question} [Y/n] ");
    if std::io::stdout().flush().is_err() {
        return false;
    }
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "" | "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absent() -> bool {
        false
    }
    fn already_there() -> bool {
        true
    }

    #[test]
    fn flags_map_to_the_right_policy() {
        assert_eq!(Consent::from_flags(false, false), Consent::Prompt);
        assert_eq!(Consent::from_flags(true, false), Consent::AssumeYes);
        assert_eq!(Consent::from_flags(false, true), Consent::Never);
        // --no-install dominates even if both somehow arrive set.
        assert_eq!(Consent::from_flags(true, true), Consent::Never);
    }

    #[test]
    fn an_installed_tool_needs_no_consent_and_never_installs() {
        for c in [Consent::Prompt, Consent::AssumeYes, Consent::Never] {
            assert!(ensure("dummy", already_there, &["install", "dummy"], c).is_ok());
        }
    }

    #[test]
    fn no_install_reports_the_command_instead_of_installing() {
        let err = ensure("dummy", absent, &["install", "dummy"], Consent::Never).unwrap_err();
        assert!(err.to_string().contains("cargo install dummy"));
        assert!(err.to_string().contains("--no-install"));
    }

    #[test]
    fn a_prompt_with_no_terminal_declines_rather_than_installing() {
        // `cargo test` runs with stdin detached from a tty, so `confirm` returns
        // false and `ensure` must bail WITHOUT attempting a `cargo install`.
        let err = ensure("dummy", absent, &["install", "dummy"], Consent::Prompt).unwrap_err();
        assert!(err.to_string().contains("--yes"));
    }
}
