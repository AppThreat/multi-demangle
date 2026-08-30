# Vendored patch: scala-native-demangle 0.0.6

This directory is the crate source of
[`scala-native-demangle` 0.0.6](https://crates.io/crates/scala-native-demangle)
(the latest release at the time of writing) with one local fix, wired in via
`[patch.crates-io]` in the root `Cargo.toml`.

## Why

The `name` parser took a declared length prefix at face value and sliced the
input with it:

```rust
return Ok((length.len() + res, rest[0..res].to_string()));
```

A crafted `_S` symbol whose declared length exceeds the bytes that remain
panics with a slice-out-of-bounds. The Plan 05 fuzz target (`fuzz/`) found
this on its first `demangle` run with the input

```
_ST31_sEquatabSg0F0QzFTW
```

(31 declared, 19 remaining), reached from this crate's unknown-language
fallback chain. Demanglers parse untrusted input by definition, so a
dependency panic is this crate's bug to stop.

The fix makes the length-bounded slices fallible: when the declared length
exceeds the remaining input, parsing fails with `name: invalid length`
instead of panicking. Nothing else is changed.

A later fuzz run (detect target, artifact
crash-8081a7ab43a8322d6ffd3bfdf46eddd6e8a43a80) found two more
overflow sites in `sig_name`: with an empty type-name list, the
`type_names[0..n - 2]` / `[0..n - 1]` renders underflow `n - 2` / `n - 1`
(panics with overflow checks on), and the `D` branch sliced
`after_name[consumed + 1..]` past the end of the input. Empty lists and
truncated input now fail with a string error like the parser's existing
rejections; every non-empty input renders exactly as before. A third run
found the same class in `read_type_names`, whose `input[pos..]` scan
panicked when a type name's consumed length ran past the input and
looped forever on a zero-consumption type; the scan now rejects both.

## Maintenance

- Every other unchecked slice in the crate was left as found; continued
  fuzzing is the check that none of the others is reachable. A new finding
  gets the same treatment: minimal fix here, regression test in `tests/`.
- When upstream publishes a release with the length fix, delete this
  directory and the `[patch.crates-io]` section, and bump the dependency
  version. `cargo update` after that must show the registry source, not this
  path.
