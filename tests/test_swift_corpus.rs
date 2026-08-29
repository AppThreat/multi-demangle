//! Per-toolchain Swift mangling corpus snapshots.
//!
//! `scripts/collect-swift-corpus.sh` compiles a fixture with a concrete Swift
//! toolchain and stores its mangled symbols under
//! `tests/corpus/swift/<version>/symbols.txt`. This test pins how the vendored
//! demangler renders that toolchain's output, so a vendor sync that changes
//! rendering — or a toolchain emitting node kinds the demangler does not know
//! yet — shows up as a reviewable diff. This is what makes the README's
//! "up to Swift X" claim testable rather than aspirational.
//!
//! When a diff is expected (e.g. right after a sync), regenerate the
//! snapshots deliberately and review them:
//!
//! ```ignore
//! MULTI_DEMANGLE_UPDATE_SNAPSHOTS=1 cargo test --all-features --test test_swift_corpus
//! ```
#![cfg(feature = "swift")]

use std::fs;
use std::path::{Path, PathBuf};

use multi_demangle::{Demangle, DemangleOptions};
use symbolic_common::{Language, Name, NameMangling};

/// Set to rewrite `expected.txt` files instead of comparing against them.
const UPDATE_ENV: &str = "MULTI_DEMANGLE_UPDATE_SNAPSHOTS";

/// Placeholder line for symbols the demangler rejects.
const FAILED_MARK: &str = "<demangling failed>";

/// Collects `tests/corpus/swift/*/` directories that hold a `symbols.txt`,
/// sorted by version name.
fn corpus_dirs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("swift");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_dir() && path.join("symbols.txt").is_file())
        .collect();
    dirs.sort();
    dirs
}

/// Demangles every symbol (complete options); one output line per input line.
fn render_all(symbols: &[String]) -> Vec<String> {
    let opts = DemangleOptions::complete();
    symbols
        .iter()
        .map(|sym| {
            Name::new(sym, NameMangling::Mangled, Language::Swift)
                .demangle(opts)
                .unwrap_or_else(|| FAILED_MARK.to_string())
        })
        .collect()
}

#[test]
fn test_swift_per_version_snapshots() {
    let dirs = corpus_dirs();
    if dirs.is_empty() {
        eprintln!("no per-version Swift corpus present; skipping");
        return;
    }

    let updating = std::env::var_os(UPDATE_ENV).is_some();
    let mut failures: Vec<String> = Vec::new();

    for dir in dirs {
        let version = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let symbols: Vec<String> = fs::read_to_string(dir.join("symbols.txt"))
            .expect("symbols.txt is readable")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(String::from)
            .collect();

        let rendered = render_all(&symbols);

        // Sanity floor even in update mode: a sync or toolchain bump that
        // makes most of a corpus fail must be investigated, not snapshotted.
        let demangled_count = rendered
            .iter()
            .filter(|line| line.as_str() != FAILED_MARK)
            .count();
        assert!(
            demangled_count * 2 > rendered.len(),
            "corpus {version}: only {demangled_count}/{} symbols demangle",
            rendered.len()
        );

        let expected_path = dir.join("expected.txt");
        if updating {
            fs::write(&expected_path, format!("{}\n", rendered.join("\n")))
                .expect("expected.txt is writable");
            eprintln!("corpus {version}: wrote {}", expected_path.display());
            continue;
        }

        let expected = match fs::read_to_string(&expected_path) {
            Ok(content) => content,
            Err(_) => {
                failures.push(format!(
                    "corpus {version}: missing expected.txt — run with {UPDATE_ENV}=1 to create it"
                ));
                continue;
            }
        };

        let expected_lines: Vec<&str> = expected.lines().collect();
        if expected_lines != rendered.iter().map(String::as_str).collect::<Vec<_>>() {
            // Walk the longer of the two so a purely-length difference (symbols
            // added or removed by a toolchain bump) still reports the offending
            // lines instead of an empty diff block.
            let missing = "<missing>";
            let diffs: Vec<String> = (0..expected_lines.len().max(rendered.len()))
                .map(|idx| {
                    (
                        idx,
                        expected_lines.get(idx).copied().unwrap_or(missing),
                        rendered.get(idx).map(String::as_str).unwrap_or(missing),
                    )
                })
                .filter(|(_, expected, actual)| expected != actual)
                .take(5)
                .map(|(idx, expected, actual)| {
                    let symbol = symbols.get(idx).map(String::as_str).unwrap_or(missing);
                    format!(
                        "  [{idx}] {symbol}\n     expected: {expected}\n     actual:   {actual}"
                    )
                })
                .collect();
            failures.push(format!(
                "corpus {version}: snapshot mismatch ({} vs {} lines)\n{}\n  ... run with {UPDATE_ENV}=1 after reviewing the diff",
                expected_lines.len(),
                rendered.len(),
                diffs.join("\n")
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "Swift corpus snapshots failed:\n\n{}\n",
        failures.join("\n\n")
    );
}
