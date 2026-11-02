//! Writes the version of the compiler and the target into `ATELIER_RUSTC`: the IDE loads a
//! native plugin only if both were built by the same compiler, for the same target.

use std::{env, process::Command};

fn main() {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = Command::new(rustc)
        .arg("-V")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let target = env::var("TARGET").unwrap_or_default();
    let profile = env::var("PROFILE").unwrap_or_default();
    println!("cargo:rustc-env=ATELIER_RUSTC={version} {target} {profile}");
    println!("cargo:rerun-if-env-changed=RUSTC");
}
