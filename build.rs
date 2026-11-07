fn git(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn source_identity() {
    let sha = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain", "--untracked-files=all"]).is_none_or(|status| !status.is_empty());
    println!("cargo:rustc-env=DEDUCTUM_BUILD_SHA={sha}");
    println!("cargo:rustc-env=DEDUCTUM_BUILD_STATE={}", if dirty { "dirty" } else { "clean" });
    for path in ["src", "res", "deductum", "Cargo.toml", "Cargo.lock", "build.rs"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let mut refs = vec!["HEAD".to_string(), "index".to_string()];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        refs.push(branch);
    }
    for name in refs {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn main() {
    source_identity();
}
