//! Real-toolchain corpus tests for the D, Fortran, Kotlin/Native, and Ada
//! backends (Plan 07/08).
//!
//! The symbols come from `contrib/`: the fixtures there are compiled inside
//! toolchain images and the resulting `nm` dumps are committed as
//! `tests/corpus/<lang>_symbols.txt` with a provenance file. These are
//! symbols a real compiler actually emitted, so — unlike table-driven unit
//! tests — they cannot encode a grammar belief the compiler does not share.
//! That circularity produced six silent bugs before this corpus existed.
//!
//! Expectations are split into two authority tiers, in separate files
//! (both `<symbol>\t<expected>`, `#` comments, `<rejected>` for a symbol the
//! pipeline must not claim):
//!
//! - **`<lang>_golden.txt`** — verified against GNU `c++filt` (D, Ada) or
//!   against the fixture's source declarations (Fortran, Kotlin/Native). A
//!   mismatch is a bug and fails CI.
//! - **`<lang>_snapshot.txt`** — merely stable: documented divergences from
//!   the reference and symbols it fails. A mismatch needs a deliberate
//!   refresh, reviewed like any snapshot update.
//!
//! Refresh the snapshot tier with:
//!
//! ```ignore
//! MULTI_DEMANGLE_UPDATE_SNAPSHOTS=1 cargo test --test test_new_language_corpus
//! ```
//!
//! D and Ada golden tiers regenerate via
//! `contrib/scripts/update-corpus-expectations.sh`; the Fortran and
//! Kotlin/Native golden files are hand-curated against the fixtures.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use multi_demangle::{Demangle, DemangleOptions};
use symbolic_common::Name;

/// Placeholder for a symbol the auto-detect pipeline must not claim.
const REJECTED: &str = "<rejected>";

/// How a symbol must render, from one expectation file.
fn load_expectations(path: &Path) -> BTreeMap<String, String> {
    let content =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    let mut map = BTreeMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((sym, expected)) = line.split_once('\t') else {
            panic!(
                "{}: line is not <symbol>\\t<expected>: {line:?}",
                path.display()
            );
        };
        map.insert(sym.to_string(), expected.to_string());
    }
    map
}

/// Renders a symbol through the real auto-detect pipeline; `None` when no
/// backend claims it.
fn render(sym: &str) -> Option<String> {
    Name::from(sym).demangle(DemangleOptions::complete())
}

/// The four new-language corpora, in test order.
const LANGUAGES: [&str; 4] = ["dlang", "ada", "fortran", "kotlin"];

fn corpus_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
}

fn read_symbols(lang: &str) -> Vec<String> {
    fs::read_to_string(corpus_dir().join(format!("{lang}_symbols.txt")))
        .unwrap_or_else(|e| panic!("{lang}_symbols.txt is readable: {e}"))
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[test]
fn test_new_language_corpus_tiers() {
    let updating = std::env::var_os("MULTI_DEMANGLE_UPDATE_SNAPSHOTS").is_some();
    let mut failures: Vec<String> = Vec::new();

    for lang in LANGUAGES {
        let symbols = read_symbols(lang);
        let golden = load_expectations(&corpus_dir().join(format!("{lang}_golden.txt")));

        // Golden tier: hard assertions. A change here is a bug.
        for (sym, expected) in &golden {
            let actual = render(sym);
            let matches = match expected.as_str() {
                REJECTED => actual.is_none(),
                expected => actual.as_deref() == Some(expected),
            };
            if !matches {
                failures.push(format!(
                    "{lang} GOLDEN mismatch: {sym}\n  expected: {expected}\n  actual:   {}",
                    actual.as_deref().unwrap_or(REJECTED)
                ));
            }
        }

        // Snapshot tier: record-or-compare.
        let snapshot_path = corpus_dir().join(format!("{lang}_snapshot.txt"));
        let snapshot = load_expectations(&snapshot_path);
        let mut current: BTreeMap<String, String> = BTreeMap::new();
        for sym in &symbols {
            if golden.contains_key(sym) {
                continue;
            }
            let rendered = render(sym).unwrap_or_else(|| REJECTED.to_string());
            current.insert(sym.clone(), rendered);
        }
        if updating {
            let mut out = String::from(
                "# Snapshot tier: stable output, refreshed deliberately.\n\
                 # Regenerate with MULTI_DEMANGLE_UPDATE_SNAPSHOTS=1 cargo test \
                 --test test_new_language_corpus\n# and review the diff.\n",
            );
            for (sym, rendering) in &current {
                out.push_str(sym);
                out.push('\t');
                out.push_str(rendering);
                out.push('\n');
            }
            fs::write(&snapshot_path, out)
                .unwrap_or_else(|e| panic!("{} is writable: {e}", snapshot_path.display()));
            eprintln!("{lang}: wrote {}", snapshot_path.display());
        } else {
            for (sym, expected) in &snapshot {
                let actual = current
                    .get(sym)
                    .expect("snapshot symbol is in the corpus (checked below)");
                if expected != actual {
                    failures.push(format!(
                        "{lang} snapshot mismatch (refresh deliberately): {sym}\n  expected: \
                         {expected}\n  actual:   {actual}"
                    ));
                }
            }
        }

        // Coverage: every corpus symbol is tiered exactly once, and no
        // expectation file references a symbol outside the corpus.
        for sym in &symbols {
            let in_golden = golden.contains_key(sym);
            let in_snapshot = snapshot.contains_key(sym) || current.contains_key(sym);
            if !in_golden && !in_snapshot {
                failures.push(format!("{lang}: {sym} is in neither tier"));
            }
            if in_golden && snapshot.contains_key(sym) {
                failures.push(format!("{lang}: {sym} is in both tiers"));
            }
        }
        for sym in snapshot.keys() {
            if !symbols.contains(sym) {
                failures.push(format!(
                    "{lang}: snapshot symbol {sym} is not in the corpus"
                ));
            }
        }
        for sym in golden.keys() {
            if !symbols.contains(sym) {
                failures.push(format!("{lang}: golden symbol {sym} is not in the corpus"));
            }
        }

        // Sanity floor: most of a real corpus must demangle. A change that
        // silently rejects half a corpus is investigated, not snapshotted.
        let demangled = symbols.iter().filter(|sym| render(sym).is_some()).count();
        assert!(
            demangled * 2 > symbols.len(),
            "{lang}: only {demangled}/{} corpus symbols demangle",
            symbols.len()
        );
    }

    assert!(
        failures.is_empty(),
        "new-language corpus tests failed:\n\n{}\n",
        failures.join("\n\n")
    );
}
