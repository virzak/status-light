// Included by the boards' build.rs: sets FIRMWARE_VERSION, which the board
// reports in its status line (PROTOCOL.md). Same form as the router packages'
// version, the last commit's UTC date and time then its hash
// (YYYY.MM.DD.HHMMSS~hash8), plus +dirty when tracked files have uncommitted
// changes; "unknown" outside a git checkout.

fn git(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git").args(args).env("TZ", "UTC0").output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn firmware_version() {
    let version = match (
        git(&["log", "-1", "--format=%cd", "--date=format-local:%Y.%m.%d.%H%M%S"]),
        git(&["rev-parse", "--short=8", "HEAD"]),
    ) {
        (Some(date), Some(hash)) if !date.is_empty() => {
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
            format!("{date}~{hash}{}", if dirty { "+dirty" } else { "" })
        }
        _ => "unknown".into(),
    };
    println!("cargo:rustc-env=FIRMWARE_VERSION={version}");

    // Rerun on a commit or checkout (HEAD, its log), on staging (index), and
    // on edits to the board's or the shared crates' sources (for +dirty).
    for path in ["HEAD", "logs/HEAD", "index"] {
        if let Some(p) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={p}");
        }
    }
    for dir in ["src", "../status-protocol/src", "../strip/src", "../flame/src"] {
        println!("cargo:rerun-if-changed={dir}");
    }
}
