# Vendored Swift demangler subset

This directory intentionally contains only the minimal subset of Swift/LLVM sources
needed to build the Swift demangler used by `multi-demangle`.

The tree was originally imported from upstream Swift to add support for newer Swift
mangling formats, but most of the full Swift project is not required here. The kept
files are limited to:

- the Swift demangler translation units compiled from `build.rs`
- their transitive headers/`.def` files under `vendor/swift/include`
- the upstream license files

The exact kept file list is checked in as `vendor/swift/MANIFEST.txt`.

If you update the vendored Swift sources again, re-derive the file set from the actual
compiler dependency graph and verify with:

- `cargo test --all-features`
- `maturin develop --all-features && pytest python/tests`
- optional packaging checks such as `maturin build` / `maturin sdist`
