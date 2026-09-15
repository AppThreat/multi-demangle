# Swift vendor sync log

Records of every `vendor/swift` sync from upstream Swift, produced by
`scripts/sync-swift.sh <swift-tag>` (run it rather than editing by hand).
Each entry lists the upstream refs, what changed in the vendored subset,
the diffstat, and the validation results. The `sync-metadata` block at the
bottom always describes the most recent sync; the monthly CI reminder
workflow parses it to list upstream commits since the last sync.

## 2026-08-29 — swift-6.3.3-RELEASE

- Swift ref: swift-6.3.3-RELEASE (064859e41d68596f486c5d724401cb370f260409)
- LLVM ref: swift-6.3.3-RELEASE (82cdc19fa54d566969527b56f587ea8ea30bef51)
- Headers added to the manifest: none
- Manifest files missing upstream: none
- Diffstat:  18 files changed, 3023 insertions(+), 1481 deletions(-)
- New files:
  vendor/swift/SYNC.md
  vendor/swift/include/llvm-c/
  vendor/swift/include/llvm/ADT/ADL.h
  vendor/swift/include/llvm/ADT/DenseMapInfo.h
  vendor/swift/include/llvm/ADT/STLForwardCompat.h
  vendor/swift/include/llvm/ADT/STLFunctionalExtras.h
  vendor/swift/include/llvm/ADT/bit.h
  vendor/swift/include/llvm/Config/
  vendor/swift/include/llvm/Support/DataTypes.h
- Validation: cargo test --all-features PASS; pytest PASS; ASan/UBSan corpus PASS


## 2026-09-15 — swift-6.4.0-RELEASE

- Swift ref: swift-6.4.0-RELEASE (b8189d766d86ad7fc8106787d6ce9e402f38dd72)
- LLVM ref: swift-6.4.0-RELEASE (903b9faaae5c43ecc9b7e33f8db9c94c7429374a)
- Headers added to the manifest: none
- Manifest files missing upstream: lib/Demangling/Errors.cpp — renamed upstream
  to lib/Demangling/DemanglingErrorHandling.cpp (byte-identical modulo the
  header comment); vendored under the new name, dropped the old one, and
  updated build.rs/scripts accordingly
- Diffstat:  10 files changed, 137 insertions(+), 26 deletions(-)
- Mangling changes: Read2Accessor/Modify2Accessor renamed to
  YieldingBorrowAccessor/YieldingMutateAccessor (printed `yielding_borrow`/
  `yielding_mutate`, SE-0474); new BuiltinBorrow node (`BW`, SE-0507);
  new ImplNonisolatedNonsendingIsolation impl-function-type flag (`N`,
  printed `@caller_isolated`); new EscapingClosureProp function-signature
  specialization parameter (`E`); inverse requirements on associated types
  (`j`/`J`)
- New files:
  vendor/swift/lib/Demangling/DemanglingErrorHandling.cpp
- Validation: cargo test --all-features PASS; pytest PASS (run after the
  sync — the script skipped it because the venv lacked maturin at the time);
  ASan/UBSan corpus PASS (including the new tests/corpus/swift/6.4.0 corpus);
  added per-toolchain corpus tests/corpus/swift/6.4.0 (Apple Swift 6.4.0,
  arm64-apple-macosx, with -enable-experimental-feature CoroutineAccessors
  for the fixture's SE-0474 accessors)

<!-- sync-metadata
swift-ref: swift-6.4.0-RELEASE
swift-commit: b8189d766d86ad7fc8106787d6ce9e402f38dd72
llvm-ref: swift-6.4.0-RELEASE
llvm-commit: 903b9faaae5c43ecc9b7e33f8db9c94c7429374a
date: 2026-09-15
-->
