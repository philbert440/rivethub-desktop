use std::process::Command;

/// Names the commit the app was built from, so About can tell a stale build from a current one.
fn main() {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|text| text.trim().to_owned())
    };
    for watched in ["HEAD", "refs/heads"] {
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", watched]) {
            println!("cargo::rerun-if-changed={path}");
        }
    }
    if let Some(revision) = git(&["rev-parse", "--short", "HEAD"]) {
        println!("cargo::rustc-env=RIVETHUB_DESKTOP_REVISION={revision}");
    }
}
