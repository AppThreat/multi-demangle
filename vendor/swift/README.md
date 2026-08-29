# Vendored Swift demangler subset

This directory intentionally contains only the minimal subset of Swift/LLVM sources
needed to build the Swift demangler used by `multi-demangle`.

The tree was originally imported from upstream Swift to add support for newer Swift
mangling formats, but most of the full Swift project is not required here. The kept
files are limited to:

- the Swift demangler translation units compiled from `build.rs`
- their transitive headers/`.def` files under `vendor/swift/include`
- the upstream license files
- two local stubs (`include/llvm/Config/llvm-config.h` and
  `include/llvm/Config/abi-breaking.h`) for headers upstream generates with
  CMake — they have no source version in the upstream repositories

The exact kept file list is checked in as `vendor/swift/MANIFEST.txt`.

## Syncing

Run `scripts/sync-swift.sh [swift-tag]` from the repository root. It:

1. shallow-clones apple/swift (and swiftlang/llvm-project for the LLVM headers)
   at the release tag — blobless and sparse, so the checkout stays small;
2. copies the manifest file list from those trees, and during the build probe
   automatically adds any header the demangler's dependency graph now needs;
3. regenerates `MANIFEST.txt`;
4. runs the validation gauntlet: `cargo test --all-features`, the Python tests
   (when the repo `.venv` has maturin), and an ASan/UBSan pass over the
   real-symbol corpus (this is the check that catches C++ regressions slipping
   in through a sync);
5. appends the provenance entry — refs, commits, added headers, diffstat,
   validation results — to `vendor/swift/SYNC.md`.

After the script finishes, work through its printed checklist: review the diff,
update the README's "up to Swift X" claim if the supported version moved, and —
when rendering changed — regenerate the per-toolchain corpus snapshots with
`MULTI_DEMANGLE_UPDATE_SNAPSHOTS=1 cargo test --all-features --test
test_swift_corpus` and review that diff before releasing (downstream consumers
such as blint match on the demangled strings).

A monthly CI workflow (`swift-sync-reminder`) opens an issue listing upstream
commits that touch `lib/Demangling` or `include/swift/Demangling` since the last
`SYNC.md` entry, so a due sync is never silent.

## Manual checks

If you bypass the script, re-derive the file set from the actual compiler
dependency graph and verify with:

- `cargo test --all-features`
- `maturin develop --all-features && pytest python/tests`
- optional packaging checks such as `maturin build` / `maturin sdist`
