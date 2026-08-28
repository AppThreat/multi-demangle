# Plan 06 — Swift vendor sync automation & fidelity

**Tier:** 2 · **Effort:** M · **Value:** Swift support is multi-demangle's
marquee differentiator (the main reason blint migrated from `symbolic`), and it
rots without a sync process: the vendored subset must track
[apple/swift](https://github.com/apple/swift) as new mangling node kinds and
print rules land.

## Current state

- `vendor/swift` holds a 46-file minimal subset of the Swift standard library
  demangler (manifest: `vendor/swift/MANIFEST.txt`), compiled by `build.rs`,
  reached through a thin C ABI (`src/swiftdemangle.cpp` wrappers declared at
  `src/lib.rs:52-64`).
- README states support "up to Swift 6.3".
- The vendor README documents the process manually: re-derive the file set from
  the compiler dependency graph, then validate with `cargo test --all-features`
  and the Python tests. Everything is by hand today.
- Output goes through a fixed 4 KiB buffer; symbols whose demangled form does
  not fit are **rejected** (silently become `None`) — long closure names in
  real iOS apps can plausibly hit this.

## Proposal

### 1. Sync script + provenance

`scripts/sync-swift.sh <swift-tag>`:

1. shallow-clone `apple/swift` at the tag;
2. copy the manifest file list (updating it if upstream added files to the
   demangler's dependency graph — check `lib/Demangling/*` and new includes);
3. record provenance in `vendor/swift/SYNC.md`: tag, commit, date, diffstat vs
   previous sync, and the `cargo test` result;
4. run the validation gauntlet: `cargo test --all-features`, wheel +
   `pytest python/tests`, ASan/UBSan corpus pass (Plan 05 — this is the job
   that catches C++ regressions slipping in through a sync).

Add a scheduled CI reminder (monthly) that opens an issue listing commits
touching `lib/Demangling` or `include/swift/Demangling` upstream since the
last sync — the sync stays opportunistic but never silent.

### 2. Version-pinned symbol corpus per Swift release

Extend the Plan 05 corpus with `corpus/swift/<version>/` sets extracted from
binaries built/observed per toolchain (a small script using `swiftc` on the
runner, plus system dylibs for older mangling schemes). The demangle snapshot
test then proves exactly which mangling versions a sync unlocks, and the
README's "up to Swift X" claim becomes testable rather than aspirational.

### 3. Fidelity improvements

- **Buffer policy:** raise the Swift output buffer (e.g. 64 KiB — long Swift
  closure names are real), or add a two-call grow protocol: first call returns
  required length, caller retries. Keep `BoundedString`-style caps so the
  billion-laughs bound survives. Add a corpus case that exceeds 4 KiB today to
  prove the bug exists, then fix it.
- **Feature flags parity:** `SYMBOLIC_SWIFT_FEATURE_RETURN_TYPE / _PARAMETERS`
  mirror upstream `SWIFT_DEMANGLING_…` option bits; verify none were added
  upstream (e.g. sugar/options affecting `async`/actor rendering) as part of
  each sync checklist.
- **Structured output:** the subset already vendors `NodeDumper.cpp`; expose a
  dump entry point per Plan 04 Phase 2b so Swift structure comes from the node
  tree rather than text parsing.
- **Async/actor rendering:** confirm current output renders `async`,
  `@Sendable`, global-actor and concurrency elements the way recent Xcode tools
  do (see the
  [SwiftDemangle](https://swiftpackageindex.com/oozoofrog/SwiftDemangle)
  package and upstream
  [swift-demangle tool](https://github.com/apple/swift/blob/main/tools/swift-demangle/swift-demangle.cpp)
  for reference behavior); add snapshot cases for each.
- **Watch upstream API direction:** Swift evolution is discussing exposing the
  runtime demangler in-process
  ([pitch](https://forums.swift.org/t/pitch-expose-demangle-function-in-runtime-module/82605));
  no action needed now, but it may eventually offer an officially supported
  symbol demangle API to align with.

## Implementation steps

1. Write `sync-swift.sh` + `SYNC.md` provenance file; do one sync against the
   latest swift tag as a shakedown (this likely bumps "up to Swift X.Y").
2. Add the CI monthly upstream-diff reminder workflow.
3. Buffer policy fix with over-long-symbol regression test.
4. Swift per-version corpus + snapshot tests; async/actor rendering cases.
5. (Depends on Plan 04) node-dump FFI and Swift structured mapping.

## Risks & mitigations

- **Upstream refactors pulling in new dependencies** (new `.def` files,
  headers): the manifest-driven copy makes the needed additions explicit;
  build failure is the immediate signal, and the script surfaces the new
  dependency graph.
- **Sync-induced output changes breaking blint matching:** Plan 05's snapshot
  tests make any change loud before release; blint pins versions in
  `pyproject.toml` and can upgrade deliberately.
- **License drift upstream:** files stay under Apache 2.0/LLVM licensing;
  `LICENSE_LLVM.txt` is already vendored — re-verify per sync in the checklist.

## Acceptance criteria

- A single documented command performs a sync and its validation; provenance
  recorded per sync.
- Monthly upstream-diff reminder active; buffer limit no longer silently drops
  valid long symbols.
- README's supported-Swift version claim backed by per-version corpus tests.
