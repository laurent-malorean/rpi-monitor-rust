//! Marque chaque binaire avec le commit et la date de son dernier commit (affichés dans l'interface Web).

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

fn main() {
    let hash = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "inconnu".into());
    let date = git(&["log", "-1", "--format=%cd", "--date=format:%d/%m %H:%M"]).unwrap_or_default();
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    println!("cargo:rustc-env=RPI_MONITOR_BUILD={hash}{} {date}", if dirty { "+" } else { "" });
    println!("cargo:rerun-if-changed=.git/logs/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
}
