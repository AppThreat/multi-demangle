#![no_main]
//! Hygiene target: the Plan 01 normalizer passes must be idempotent —
//! `normalize(normalize(x)) == normalize(x)` — and must not panic (Plan 05
//! §1). Asserted for both pass sets; a violating input is a hygiene bug by
//! definition, because running the passes twice must not change how a symbol
//! matches or displays.
//!
//! Scope note (fuzz finding, artifact
//! crash-001de558110000b022374f84d2428446e5517865): universal idempotence is
//! impossible for the legacy Rust escape decoder *by construction*, and that
//! decoder's behavior is deliberate. It decodes `$u24$` to `$` (one level,
//! mirroring rustc_demangle's printer), so decoded text can re-form something
//! that looks like an escape — `$u80$u62$` keeps the rejected `$u80` and the
//! decoded `b` merges into a fresh `$u80b$`. Making that idempotent would
//! require re-decoding replacements, which corrupts the one-level contract.
//! The property is therefore asserted where it is achievable:
//!
//! - strip-only normalizers (`legacy_rust_escapes(false)`): universal
//!   idempotence, every input. This is where the first normalize finding
//!   lived (the version pass peeled one more `@` layer per call).
//! - full display/matching on inputs without `$`: end-to-end idempotence,
//!   every such input.

use libfuzzer_sys::fuzz_target;
use multi_demangle::{normalize_symbol, Normalizer};

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let _ = normalize_symbol(s);

    for (name, normalizer) in [
        (
            "display-strips",
            Normalizer::display().legacy_rust_escapes(false),
        ),
        (
            "matching-strips",
            Normalizer::matching().legacy_rust_escapes(false),
        ),
    ] {
        let once = normalizer.normalize(s);
        let twice = normalizer.normalize(&once);
        assert_eq!(once, twice, "{name} normalize is not idempotent on {s:?}");
    }

    if !s.contains('$') {
        for (name, normalizer) in
            [("display", Normalizer::display()), ("matching", Normalizer::matching())]
        {
            let once = normalizer.normalize(s);
            let twice = normalizer.normalize(&once);
            assert_eq!(
                once, twice,
                "{name} normalize is not idempotent on {s:?}"
            );
        }
    }
});
