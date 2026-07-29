//! Host prerequisite **detection**.
//!
//! The *data* — which tools exist, how to probe each one, how to install it —
//! lives in [`crate::registry::PREREQS`] (R4); the *selection* — which of them a
//! given project needs — lives in [`crate::model::prerequisites`]. This module
//! only answers "is it actually here?", and it never guesses: every answer comes
//! from running the probe.
//!
//! ## Cost
//!
//! Detection runs once per generation, over the handful of prerequisites the
//! resolved config selected (2–4 in practice, never the whole catalog). Each
//! probe is one short-lived process; the two rustup queries are memoised in
//! [`Host`], so a report that mentions three Rust targets still shells out to
//! `rustup` once. Measured on the development machine: `rustup target list
//! --installed` ~24 ms, `rustup component list` ~17 ms, the `esp`-toolchain
//! probe ~190 ms (and only ever runs for an Xtensa config), tool `--version`
//! calls a few ms each.

use std::collections::HashMap;
use std::process::{Command, Stdio};

use crate::registry::{Prereq, Probe};

/// Whether a prerequisite was found on this host. Deliberately two-valued: we
/// either ran a probe and it answered, or we report it missing. There is no
/// "assumed present".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Installed,
    Missing,
}

impl Status {
    /// The marker shown in the summary, matching the `✔` the summary header uses.
    pub fn marker(self) -> &'static str {
        match self {
            Status::Installed => "✔",
            Status::Missing => "✗",
        }
    }

    pub fn is_missing(self) -> bool {
        matches!(self, Status::Missing)
    }
}

/// Memoises the rustup queries so a report probes each one at most once.
#[derive(Debug, Default)]
pub struct Host {
    /// `rustup target list --installed` for the active toolchain.
    targets: Option<Vec<String>>,
    /// `rustup component list --toolchain <tc> --installed`, per toolchain.
    components: HashMap<&'static str, Vec<String>>,
    /// Marker triple → whether the `esp` toolchain can build it.
    xtensa: HashMap<&'static str, bool>,
}

impl Host {
    pub fn new() -> Self {
        Self::default()
    }

    /// Detect one prerequisite. Every branch runs a real probe.
    pub fn detect(&mut self, p: &Prereq) -> Status {
        let found = match p.probe {
            Probe::Binary(argv) => spawns(argv),
            Probe::RustupTarget(triple) => self.targets().iter().any(|t| t == triple),
            Probe::RustupComponent { toolchain, component } => self
                .components(toolchain)
                .iter()
                // rustup prints components as bare names (`rust-src`) or with a
                // host suffix (`rustc-aarch64-apple-darwin`); match both.
                .any(|c| c == component || c.starts_with(&format!("{component}-"))),
            Probe::XtensaToolchain(marker) => self.xtensa_can_build(marker),
        };
        if found {
            Status::Installed
        } else {
            Status::Missing
        }
    }

    fn targets(&mut self) -> &[String] {
        if self.targets.is_none() {
            self.targets = Some(lines(&["rustup", "target", "list", "--installed"]));
        }
        self.targets.as_deref().unwrap_or(&[])
    }

    fn components(&mut self, toolchain: &'static str) -> &[String] {
        self.components.entry(toolchain).or_insert_with(|| {
            lines(&["rustup", "component", "list", "--toolchain", toolchain, "--installed"])
        })
    }

    /// Whether rustup has an `esp` toolchain whose rustc can build `marker`.
    ///
    /// Custom (espup-linked) toolchains do not report their targets through
    /// `rustup target list --toolchain esp`, so the only honest question is the
    /// one asked here: run that toolchain's own rustc and see whether the triple
    /// is in its target list. A missing `esp` toolchain makes `rustup run` fail,
    /// which reads as "not installed" — correct.
    fn xtensa_can_build(&mut self, marker: &'static str) -> bool {
        if let Some(known) = self.xtensa.get(marker) {
            return *known;
        }
        let found = lines(&["rustup", "run", "esp", "rustc", "--print", "target-list"])
            .iter()
            .any(|t| t == marker);
        self.xtensa.insert(marker, found);
        found
    }
}

/// Run `argv` and return its stdout as trimmed, non-empty lines. An empty vec
/// means the command could not be run or failed — both of which mean "we learnt
/// nothing", which callers treat as "not installed".
fn lines(argv: &[&str]) -> Vec<String> {
    let Some((exe, args)) = argv.split_first() else {
        return Vec::new();
    };
    let Ok(out) = Command::new(exe).args(args).stdin(Stdio::null()).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Whether `argv` names an executable that could actually be run.
///
/// The exit status is **deliberately ignored**: `picotool --version` exits 255
/// and `ldproxy` exits 101 for every invocation (it is a linker shim with no
/// CLI), yet both are installed and working. What we want to know is exactly
/// what `command -v` answers — does this executable resolve on `PATH` — and
/// `Command::output` distinguishes that (an `Err`, typically `NotFound`) from a
/// program that ran and disliked its arguments (`Ok` with a bad status).
///
/// stdio is captured and stdin nulled so a probe can neither print over the
/// summary nor block waiting for input.
fn spawns(argv: &[&str]) -> bool {
    let Some((exe, args)) = argv.split_first() else {
        return false;
    };
    Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{Need, Probe};

    fn p(probe: Probe) -> Prereq {
        Prereq {
            id: "test",
            display: "test",
            probe,
            install: "n/a",
            need: Need::Build,
            note: None,
        }
    }

    /// Detection must be REAL: a tool that is certainly present reads as
    /// installed, and one that certainly is not reads as missing. `cargo` is
    /// guaranteed to exist — it is running this test.
    #[test]
    fn binary_probe_reports_installed_and_missing_truthfully() {
        let mut host = Host::new();
        assert_eq!(
            host.detect(&p(Probe::Binary(&["cargo", "--version"]))),
            Status::Installed
        );
        assert_eq!(
            host.detect(&p(Probe::Binary(&[
                "xiao-generate-no-such-binary-8f3a1c",
                "--version"
            ]))),
            Status::Missing
        );
    }

    /// The reason `spawns` ignores exit status. `false` is POSIX-guaranteed to
    /// exist and to exit 1 — an "installed tool that exits non-zero", which is
    /// exactly the picotool/ldproxy shape. If this ever regressed to checking
    /// the status, every ldproxy check would report a false MISSING.
    #[test]
    fn a_tool_that_exits_non_zero_is_still_installed() {
        let mut host = Host::new();
        assert_eq!(host.detect(&p(Probe::Binary(&["false"]))), Status::Installed);
    }

    #[test]
    fn rustup_target_probe_reports_missing_for_an_unknown_triple() {
        let mut host = Host::new();
        assert_eq!(
            host.detect(&p(Probe::RustupTarget("not-a-real-target-triple"))),
            Status::Missing
        );
    }

    /// The host's own target is always installed, so this pins that the probe can
    /// say "installed" too — a probe that only ever returned Missing would pass
    /// the test above while being useless.
    ///
    /// The expected triple comes from `rustc -vV`, i.e. NOT from the same
    /// `rustup target list` output the probe reads, so this is a real
    /// cross-check rather than a tautology.
    #[test]
    fn rustup_target_probe_finds_the_host_target() {
        let Ok(out) = Command::new("rustc").arg("-vV").output() else {
            eprintln!("skipping: rustc not runnable");
            return;
        };
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let Some(host_triple) = text
            .lines()
            .find_map(|l| l.strip_prefix("host: "))
            .map(|h| h.trim().to_string())
        else {
            eprintln!("skipping: `rustc -vV` reported no host triple");
            return;
        };
        let mut host = Host::new();
        // `RustupTarget` holds a `&'static str`; leak the triple (test-only).
        let leaked: &'static str = Box::leak(host_triple.into_boxed_str());
        assert_eq!(host.detect(&p(Probe::RustupTarget(leaked))), Status::Installed);
    }

    #[test]
    fn rustup_component_probe_reports_missing_for_an_unknown_toolchain() {
        let mut host = Host::new();
        assert_eq!(
            host.detect(&p(Probe::RustupComponent {
                toolchain: "xiao-generate-no-such-toolchain",
                component: "rust-src",
            })),
            Status::Missing
        );
    }

    #[test]
    fn xtensa_probe_reports_missing_when_the_marker_is_not_buildable() {
        // Whatever `esp` toolchain the host has (or has not), it cannot build a
        // nonsense triple — so this must be Missing, never an assumed yes.
        let mut host = Host::new();
        assert_eq!(
            host.detect(&p(Probe::XtensaToolchain("not-a-real-target-triple"))),
            Status::Missing
        );
    }

    /// Repeated detection must not re-shell out — the memoisation is what keeps
    /// a multi-target report cheap.
    #[test]
    fn rustup_queries_are_memoised() {
        let mut host = Host::new();
        host.detect(&p(Probe::RustupTarget("not-a-real-target-triple")));
        assert!(host.targets.is_some(), "the target list must be cached after one probe");
        host.detect(&p(Probe::RustupComponent {
            toolchain: "xiao-generate-no-such-toolchain",
            component: "rust-src",
        }));
        assert_eq!(host.components.len(), 1);
        host.detect(&p(Probe::XtensaToolchain("not-a-real-target-triple")));
        assert_eq!(host.xtensa.len(), 1);
    }

    /// Every catalog entry must be probeable without panicking, and the probe
    /// argv must be non-empty (an empty argv would silently always report
    /// Missing).
    #[test]
    fn every_catalog_prereq_can_be_probed() {
        let mut host = Host::new();
        for pr in crate::registry::PREREQS {
            if let Probe::Binary(argv) = pr.probe {
                assert!(!argv.is_empty(), "{} has an empty probe argv", pr.id);
            }
            let _ = host.detect(pr);
        }
    }
}
