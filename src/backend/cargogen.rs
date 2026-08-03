//! cargo-generate backend (M5, ESP `std`/`esp-idf` stack). Drives a **pinned**
//! `esp-rs/esp-idf-template` (the `cargo` sub-template) fully non-interactively
//! via `cargo generate -d …` (D1/D7). The template ref is pinned by commit SHA
//! (the repo has no releases/tags — see research/esp-idf-template.md); the
//! `mcu`/`espidfver` placeholders come from the registry (D2).
//!
//! Verification is generation-only (R9): we never `cargo check` the generated
//! std project — that would pull and build the full ESP-IDF SDK. We rely on
//! esp-idf-template to emit a buildable base and the user's ESP-IDF toolchain to
//! build it.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use tempfile::TempDir;

use crate::backend::bootstrap::Consent;
use crate::model::Config;
use crate::registry::{
    CARGO_GENERATE_MIN_VERSION, ESP_IDF_TEMPLATE_GIT, ESP_IDF_TEMPLATE_REV, ESP_IDF_VERSION,
};

/// Check `cargo generate` is installed and report its version. Missing tool → a
/// clear, actionable error (headless: we fail with the install command).
pub fn preflight() -> Result<String> {
    let out = Command::new("cargo")
        .args(["generate", "--version"])
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "could not run `cargo generate` ({e}). Install it with:\n    \
                 cargo install cargo-generate"
            )
        })?;
    if !out.status.success() {
        bail!(
            "`cargo generate --version` failed — install cargo-generate (>= {CARGO_GENERATE_MIN_VERSION}) with:\n    \
             cargo install cargo-generate"
        );
    }
    // "cargo generate-generate 0.22.0" → last whitespace token is the version.
    let ver = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .last()
        .unwrap_or("")
        .to_string();
    Ok(ver)
}

/// Run a `git` subcommand inside `dir`, surfacing stderr verbatim on failure.
///
/// A missing `git` is reported with an actionable message. Note that the ESP
/// `std` stack already requires `git` at *build* time (esp-idf-sys clones the
/// ESP-IDF SDK itself), so this adds no new practical burden on the user.
fn git(dir: &Path, args: &[&str]) -> Result<std::process::Output> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "could not run `git` ({e}). xiao-generate needs git on PATH to fetch the pinned \
                 esp-idf-template revision (the generated project's ESP-IDF build needs it too)."
            )
        })?;
    if !out.status.success() {
        bail!(
            "`git {}` failed:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out)
}

/// Materialize exactly `rev` of `url` into a throwaway local checkout.
///
/// # Why we fetch the template ourselves
///
/// The obvious spelling — `cargo generate --git <url> --rev <SHA>` — is
/// **broken for any revision that is not the remote's current HEAD**.
/// cargo-generate clones at `depth 1` and only *then* runs `revparse` for the
/// requested revision, so a shallow clone that does not contain the commit
/// fails with `revspec '<SHA>' not found; class=Reference (4); code=NotFound
/// (-3)`. Our pin worked only until upstream landed one more commit on
/// `master`, at which point *every* `--std` generation started failing
/// (cargo-generate#1434; fixed upstream only in cargo-generate 0.23.14).
///
/// Bumping the pin to the new HEAD is not a fix — it re-breaks on the very next
/// upstream commit. Instead we do the one thing that makes a commit pin mean
/// what it says: fetch that exact object ourselves.
///
/// `git fetch --depth 1 origin <SHA>` asks the server for precisely the pinned
/// commit, so it costs the **same single-commit transfer** as cargo-generate's
/// own shallow clone while resolving regardless of where the branch has moved.
/// The checkout is then handed to `cargo generate --path`, which produces
/// byte-identical output to `--git`/`--rev` (see the equivalence note on
/// [`scaffold`]).
fn fetch_revision(url: &str, rev: &str) -> Result<TempDir> {
    let dir = tempfile::Builder::new()
        .prefix("xiao-generate-template-")
        .tempdir()
        .context("creating a temporary directory for the template checkout")?;
    let p = dir.path();

    git(p, &["init", "--quiet"])?;
    git(p, &["remote", "add", "origin", url])?;
    // Fetching the SHA directly (rather than a branch) is what makes the pin
    // durable; GitHub allows it, and `--depth 1` keeps the transfer minimal.
    git(p, &["fetch", "--depth", "1", "--quiet", "origin", rev]).with_context(|| {
        format!(
            "could not fetch revision {rev} from {url}.\nThis needs network access to the \
             template repository; if you are offline, retry when connected."
        )
    })?;
    git(
        p,
        &["-c", "advice.detachedHead=false", "checkout", "--quiet", "FETCH_HEAD"],
    )?;

    // Integrity: the tree we are about to expand must be the pinned commit and
    // nothing else. Cheap, and it turns any transport/config surprise into a
    // loud failure instead of a silently different project.
    let head = git(p, &["rev-parse", "HEAD"])?;
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
    if head != rev {
        bail!("template checkout landed on {head}, expected the pinned revision {rev}");
    }
    Ok(dir)
}

pub fn scaffold(cfg: &Config, consent: Consent) -> Result<()> {
    // Install cargo-generate on demand (with consent) if it is missing.
    crate::backend::bootstrap::ensure_cargo_generate(consent)?;

    let _installed = preflight()?;

    // Pinned revision first, then expand it from disk. `cargo generate --path
    // <checkout> cargo` is equivalent to `--git <url> --rev <SHA> cargo`
    // (verified byte-for-byte on a revision that both spellings can reach) but
    // does not depend on the pin still being the branch tip.
    let template = fetch_revision(ESP_IDF_TEMPLATE_GIT, ESP_IDF_TEMPLATE_REV)?;

    let out_dir = &cfg.out_dir;
    let name = out_dir
        .file_name()
        .and_then(|s| s.to_str())
        .context("output directory has no final component to use as the project name")?;
    // cargo-generate creates <destination>/<name>, so the destination is the
    // parent of out_dir (mirrors espgen's -O parent handling).
    let parent: &Path = out_dir
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating parent directory {}", parent.display()))?;

    let mut cmd = Command::new("cargo");
    cmd.arg("generate")
        .arg("--path")
        .arg(template.path())
        .arg("cargo")
        .arg("--name")
        .arg(name)
        .arg("--define")
        .arg(format!("mcu={}", cfg.variant.chip))
        // `advanced=true` unlocks espidfver; we pin it explicitly (D2/R5).
        .arg("--define")
        .arg("advanced=true")
        .arg("--define")
        .arg(format!("espidfver={ESP_IDF_VERSION}"))
        .arg("--silent")
        .arg("--vcs")
        .arg("none")
        .arg("--destination")
        .arg(parent);

    let output = cmd.output().context("failed to spawn `cargo generate`")?;
    if !output.status.success() {
        bail!(
            "cargo generate failed (mcu {}, template {}@{}):\n{}",
            cfg.variant.chip,
            ESP_IDF_TEMPLATE_GIT,
            ESP_IDF_TEMPLATE_REV,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_available() -> bool {
        Command::new("git").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    fn run(dir: &Path, args: &[&str]) {
        let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}:\n{}", String::from_utf8_lossy(&out.stderr));
    }

    /// The regression guard for the shallow-clone bug this module works around.
    ///
    /// It builds a two-commit repository and asks [`fetch_revision`] for the
    /// **first** commit — i.e. a revision that is deliberately *not* HEAD, which
    /// is exactly the case `cargo generate --git … --rev` cannot serve from its
    /// `depth 1` clone. Entirely local (a `file://` remote), so it needs no
    /// network and cannot be perturbed by upstream moving on.
    #[test]
    fn pinned_revision_resolves_when_it_is_not_head() {
        if !git_available() {
            eprintln!("skipping: git not installed");
            return;
        }
        let src = tempfile::tempdir().unwrap();
        let p = src.path();
        run(p, &["init", "--quiet", "-b", "main", "."]);
        run(p, &["config", "user.email", "test@example.invalid"]);
        run(p, &["config", "user.name", "test"]);
        // Serving an arbitrary SHA to a fetch is opt-in for a local remote;
        // GitHub (our real remote) enables it by default.
        run(p, &["config", "uploadpack.allowAnySHA1InWant", "true"]);

        std::fs::write(p.join("marker.txt"), "pinned\n").unwrap();
        run(p, &["add", "-A"]);
        run(p, &["commit", "--quiet", "-m", "pinned"]);
        let pinned = String::from_utf8(
            Command::new("git").current_dir(p).args(["rev-parse", "HEAD"]).output().unwrap().stdout,
        )
        .unwrap()
        .trim()
        .to_string();

        // Move the branch on, so `pinned` is now HEAD~1 — the situation that
        // broke every `--std` generation in the field.
        std::fs::write(p.join("marker.txt"), "moved on\n").unwrap();
        run(p, &["add", "-A"]);
        run(p, &["commit", "--quiet", "-m", "later"]);
        let head = String::from_utf8(
            Command::new("git").current_dir(p).args(["rev-parse", "HEAD"]).output().unwrap().stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        assert_ne!(pinned, head, "test setup: the pin must not be HEAD");

        let url = format!("file://{}", p.display());
        let checkout = fetch_revision(&url, &pinned).expect("pinned non-HEAD revision must resolve");

        // The content is the pinned commit's, not the branch tip's.
        let marker = std::fs::read_to_string(checkout.path().join("marker.txt")).unwrap();
        assert_eq!(marker, "pinned\n", "checkout must hold the pinned revision's tree");
    }

    /// A revision that does not exist must fail loudly rather than silently
    /// falling back to the branch tip.
    #[test]
    fn unknown_revision_is_an_error() {
        if !git_available() {
            eprintln!("skipping: git not installed");
            return;
        }
        let src = tempfile::tempdir().unwrap();
        let p = src.path();
        run(p, &["init", "--quiet", "-b", "main", "."]);
        run(p, &["config", "user.email", "test@example.invalid"]);
        run(p, &["config", "user.name", "test"]);
        std::fs::write(p.join("f.txt"), "x\n").unwrap();
        run(p, &["add", "-A"]);
        run(p, &["commit", "--quiet", "-m", "only"]);

        let url = format!("file://{}", p.display());
        let err = fetch_revision(&url, "0000000000000000000000000000000000000000").unwrap_err();
        assert!(
            format!("{err:#}").contains("could not fetch revision"),
            "expected a fetch error, got: {err:#}"
        );
    }
}
