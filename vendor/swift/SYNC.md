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

<!-- sync-metadata
swift-ref: swift-6.3.3-RELEASE
swift-commit: 064859e41d68596f486c5d724401cb370f260409
llvm-ref: swift-6.3.3-RELEASE
llvm-commit: 82cdc19fa54d566969527b56f587ea8ea30bef51
date: 2026-08-29
-->
