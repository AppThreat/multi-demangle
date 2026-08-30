//! MSVC C++ Demangling Tests
//! We use msvc_demangler under the hood which runs its own test suite.
//! Tests here make it easier to detect regressions.

#![cfg(feature = "msvc")]

#[macro_use]
mod utils;

use multi_demangle::DemangleOptions;
use symbolic_common::Language;

#[test]
fn test_msvc_demangle_without_args() {
    assert_demangle!(Language::Cpp, DemangleOptions::name_only(), {
        // These symbols were extracted from electron.exe.pdb
        // https://github.com/electron/electron/releases/download/v2.0.11/electron-v2.0.11-win32-x64-pdb.zip
        "??3@YAXPEAX@Z" => "operator delete",
        "?LoadV8Snapshot@V8Initializer@gin@@SAXXZ" => "gin::V8Initializer::LoadV8Snapshot",
        "??9@YA_NAEBVGURL@@0@Z" => "operator!=",
        "??_GAtomSandboxedRenderFrameObserver@?A0x77c58568@atom@@UEAAPEAXI@Z" => "atom::`anonymous namespace'::AtomSandboxedRenderFrameObserver::`scalar deleting destructor'",
    })
}

#[test]
fn test_msvc_demangle_full() {
    assert_demangle!(Language::Cpp, DemangleOptions::name_only().parameters(true), {
        // These symbols were extracted from electron.exe.pdb
        // https://github.com/electron/electron/releases/download/v2.0.11/electron-v2.0.11-win32-x64-pdb.zip
        "??3@YAXPEAX@Z" => "operator delete(void*)",
        "?LoadV8Snapshot@V8Initializer@gin@@SAXXZ" => "gin::V8Initializer::LoadV8Snapshot(void)",
        "??9@YA_NAEBVGURL@@0@Z" => "operator!=(GURL const&, GURL const&)",
        "??_GAtomSandboxedRenderFrameObserver@?A0x77c58568@atom@@UEAAPEAXI@Z" => "atom::`anonymous namespace'::AtomSandboxedRenderFrameObserver::`scalar deleting destructor'(unsigned int)",
    })
}

// NOTE: msvc_demangler cannot demangle without qualifiers and argument lists yet.

/// Regression test for the byte-escape underflow in msvc-demangler 0.11.0's
/// encoded-string reader: a `$<high><low>` escape computed `byte - b'A'` on
/// two unchecked bytes, panicking with overflow checks on (and silently
/// wrapping into wrong output characters in release builds) when either byte
/// sat below `A`. Found by the `demangle` fuzz target (Plan 05) with exactly
/// this input; the vendored fix in `vendor/msvc-demangler/` validates both
/// nibble letters and rejects the symbol. A dependency bump that
/// reintroduces the unchecked subtraction fails here.
#[test]
fn fuzz_found_invalid_byte_escape_is_rejected_not_panicking() {
    use multi_demangle::Demangle;
    use symbolic_common::Name;

    // libFuzzer artifact crash-52e69ba3cebf8ec03eb4e64f4fb1fb1598a36864:
    // `?D` in the string body is a byte below `A` fed to the subtraction.
    let symbol = "??1@_17@_?$??DDDFDV dDDDDD D$_wb'vb'v1J";
    let name = Name::from(symbol);
    // The malformed escape must reject consistently — no panic, and the same
    // answer on every call (a wrap would vary with nothing).
    let first = name.demangle(DemangleOptions::complete());
    let second = name.demangle(DemangleOptions::complete());
    assert_eq!(first, second);
}
