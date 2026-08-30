# Vendored patch: msvc-demangler 0.11.0

This directory is the crate source of
[`msvc-demangler` 0.11.0](https://crates.io/crates/msvc-demangler)
(the latest release at the time of writing) with one local fix, wired in via
`[patch.crates-io]` in the root `Cargo.toml` and mirrored in `fuzz/Cargo.toml`.
The vendored manifest drops the upstream bins, examples, and test targets;
their fixtures stay with upstream.

## Why

In the encoded-string reader (`??_C@…` constant strings), a byte escape
written as `$<high><low>` computed `self.get()? - b'A'` on two unchecked
bytes. Any byte below `A` panics with a subtract-underflow in builds with
overflow checks on, and silently wraps into a wrong output character in
release builds — the silent-wrong-output class this crate's symbol tables
care most about. The demangle fuzz target (Plan 05) reached it with
debug-assertions on:

    fuzz/artifacts/demangle/crash-52e69ba3cebf8ec03eb4e64f4fb1fb1598a36864

The fix validates both bytes as the letters `A`-`P` (the nibble alphabet)
and rejects the symbol otherwise. Valid encodings are unaffected.

## Maintenance

Same policy as the other `vendor/` patches: minimal fixes only; delete this
directory and both `[patch.crates-io]` sections when upstream publishes a
release with the byte validation.
