use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The crate that adapts rh's contract; every other crate works in the domain's own types.
const WIRE_ADAPTER: &str = "gateway";
const MAX_LINES: usize = 600;

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Every file under `dir` whose name ends with `suffix`, sorted.
fn files(dir: &Path, suffix: &str) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                if path.file_name().is_none_or(|name| name != "target") {
                    dirs.push(path);
                }
            } else if path.to_string_lossy().ends_with(suffix) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

fn shown(path: &Path) -> String {
    let crates = crates_dir();
    let relative = path.strip_prefix(&crates).unwrap_or(path);
    format!("crates/{}", relative.display())
}

fn is_wire_dependency(line: &str) -> bool {
    let line = line.trim_start();
    line.strip_prefix("wire")
        .is_some_and(|rest| rest.trim_start().starts_with(['=', '.']))
}

/// Only the gateway crate speaks rh's wire contract: no other crate depends on it or names it, and
/// the gateway doesn't hand its types on.
#[test]
fn only_the_gateway_crate_imports_wire() -> io::Result<()> {
    let named = concat!("wire", "::");
    let mut offenders = Vec::new();
    for manifest in files(&crates_dir(), "Cargo.toml")? {
        let in_adapter = manifest.parent().and_then(Path::file_name) == Some(OsStr::new(WIRE_ADAPTER));
        if !in_adapter && fs::read_to_string(&manifest)?.lines().any(is_wire_dependency) {
            offenders.push(format!("{} depends on wire", shown(&manifest)));
        }
    }
    for source in files(&crates_dir(), ".rs")? {
        let text = fs::read_to_string(&source)?;
        let in_adapter = source.starts_with(crates_dir().join(WIRE_ADAPTER));
        if !in_adapter && text.contains(named) {
            offenders.push(format!("{} names wire", shown(&source)));
        }
        if in_adapter
            && text
                .lines()
                .any(|line| line.trim_start().starts_with("pub use wire"))
        {
            offenders.push(format!("{} re-exports wire", shown(&source)));
        }
    }
    assert!(
        offenders.is_empty(),
        "only crates/{WIRE_ADAPTER} may import wire:\n{}",
        offenders.join("\n")
    );
    Ok(())
}

/// No source file grows past what one reader can hold at once.
#[test]
fn no_source_file_is_over_the_line_limit() -> io::Result<()> {
    let mut offenders = Vec::new();
    for source in files(&crates_dir(), ".rs")? {
        let lines = fs::read_to_string(&source)?.lines().count();
        if lines > MAX_LINES {
            offenders.push(format!("{} has {lines} lines", shown(&source)));
        }
    }
    assert!(
        offenders.is_empty(),
        "split these by responsibility, to at most {MAX_LINES} lines each:\n{}",
        offenders.join("\n")
    );
    Ok(())
}
