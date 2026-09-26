//! Stamp each compiled DLL for support purposes; no SDK dependency.
//! The stamp is a Git revision (when available) plus the actual build time.
//! Cargo reuses it when reusing an unchanged build, which is intentional.

use std::{
    env,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(env::var("CARGO_MANIFEST_DIR").ok()?)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_owned())
}

fn main() {
    // Re-run the stamp generator when source changes, not on every game launch.
    // Cargo will reuse the existing built DLL (and its stamp) if nothing changed.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");

    // Also stamp the newly committed revision when only Git metadata changed.
    if Path::new(".git/HEAD").is_file() {
        println!("cargo:rerun-if-changed=.git/HEAD");
    } else if Path::new(".git").is_file() {
        println!("cargo:rerun-if-changed=.git");
    }

    let revision = git(&["rev-parse", "--short=12", "HEAD"])
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "git-unavailable".to_owned());
    let dirty = git(&[
        "status",
        "--porcelain",
        "--untracked-files=no",
        "--",
        "src",
        "Cargo.toml",
        "build.rs",
    ])
    .map(|output| !output.is_empty())
    .unwrap_or(false);
    let built_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    let suffix = if dirty { "-dirty" } else { "" };
    println!("cargo:rustc-env=HARBINGER_BUILD_ID={revision}{suffix}-built-{built_at}");
}
