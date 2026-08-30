//! Fortran name demangling for gfortran and Intel (ifort/ifx) symbols.
//!
//! Fortran compilers flatten module procedures and variables into flat C
//! symbols; no tool in the binutils/LLVM toolchain demangles them, although
//! the encoding is purely pattern-based. The variants handled here follow
//! CMake's `FortranCInterface` detector, which is the canonical survey of the
//! real-world spellings:
//!
//! | Compiler          | Pattern                        | Demangled form       |
//! | ----------------- | ------------------------------ | -------------------- |
//! | gfortran          | `__<module>_MOD_<proc>`        | `<module>::<proc>`   |
//! | gfortran (no `__`) | `<module>_MOD_<proc>`         | `<module>::<proc>`   |
//! | gfortran, renamed | `__<module>_MOD_<proc>_<len>`  | `<module>::<proc>`   |
//! | Intel ifort/ifx   | `<module>_mp_<proc>_`          | `<module>::<proc>`   |
//!
//! The plain g77 form `<name>_` (a trailing underscore appended to a symbol
//! whose name contains no underscore) is demangled only when explicitly
//! requested through [`crate::demangle_as`]: any C symbol may end in `_`, so
//! auto-detection never claims that form.
//!
//! References: [CMake FortranCInterface](https://cmake.org/cmake/help/latest/module/FortranCInterface.html),
//! [gcc/fortran/misc.c `gfc_mangle_name`](https://github.com/gcc-mirror/gcc/blob/master/gcc/fortran/misc.c).

// Entry points are feature-gated; without the feature the module only
// provides detection predicates, and the rest is legitimately dead.
#![cfg_attr(not(feature = "fortran"), allow(dead_code))]

use crate::DemangleOptions;

/// Whether `name` is a plain Fortran identifier: ASCII letters and digits
/// (and underscores) only, starting with a letter. Fortran case-folds
/// identifiers, so gfortran emits the module part lowercase; the procedure
/// part keeps the source case.
fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name.as_bytes()[0].is_ascii_alphabetic()
}

/// A parsed Fortran symbol: the module scope (empty for the plain form) and
/// the procedure or variable name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FortranSymbol {
    /// Module name, empty when the symbol is not module-scoped.
    pub module: String,
    /// Procedure or variable name.
    pub name: String,
}

impl FortranSymbol {
    /// Renders the symbol the way Fortran source spells it.
    pub fn render(&self) -> String {
        if self.module.is_empty() {
            self.name.clone()
        } else {
            format!("{}::{}", self.module, self.name)
        }
    }
}

/// Parses the module-scoped forms: gfortran's `__<module>_MOD_<proc>` (with or
/// without the leading `__`, optionally with a trailing `_<digits>` rename
/// suffix) and Intel's `<module>_mp_<proc>_` (the trailing underscore is
/// optional on some ABIs). Returns `None` for anything else.
pub(crate) fn parse_module_symbol(symbol: &str) -> Option<FortranSymbol> {
    // Intel: `<module>_mp_<proc>_`; `_mp_` is reserved and cannot appear in a
    // Fortran identifier, so the first occurrence is the separator.
    if let Some((module, rest)) = symbol.split_once("_mp_") {
        if is_identifier(module) {
            let proc = rest.strip_suffix('_').unwrap_or(rest);
            // The procedure part may carry a trailing `_<digits>` length
            // suffix on renamed symbols, mirroring the gfortran form below.
            let proc = strip_length_suffix(proc)?;
            if is_identifier(proc) {
                return Some(FortranSymbol {
                    module: module.to_ascii_lowercase(),
                    name: proc.to_string(),
                });
            }
        }
        return None;
    }

    let bare = symbol.strip_prefix("__").unwrap_or(symbol);
    let (module, rest) = bare.split_once("_MOD_")?;
    if !is_identifier(module) {
        return None;
    }
    let proc = strip_length_suffix(rest)?;
    if is_identifier(proc) {
        return Some(FortranSymbol {
            module: module.to_ascii_lowercase(),
            name: proc.to_string(),
        });
    }
    None
}

/// Strips a trailing `_<digits>` disambiguation suffix (gfortran renames
/// symbols whose mangled form would clash, appending the length of the
/// original name), refusing empty results.
fn strip_length_suffix(name: &str) -> Option<&str> {
    let stripped = match name.rfind('_') {
        Some(idx) => {
            let digits = &name[idx + 1..];
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                &name[..idx]
            } else {
                name
            }
        }
        None => name,
    };
    if stripped.is_empty() {
        None
    } else {
        Some(stripped)
    }
}

/// Parses the plain g77 form: `<name>_` (or `<name>__` for names that already
/// contain an underscore), where the trailing underscore(s) are appended by
/// the compiler and not part of the source name.
///
/// Deliberately not part of auto-detection — a C symbol may end in `_` — so
/// this form is only demangled on explicit request (see [`crate::demangle_as`]).
pub(crate) fn parse_plain_symbol(symbol: &str) -> Option<FortranSymbol> {
    let single = symbol.strip_suffix('_')?;
    let (name, underscored) = match single.strip_suffix('_') {
        Some(base) => (base, true),
        None => (single, false),
    };
    if name.is_empty() || !is_identifier(name) {
        return None;
    }
    // The double-underscore form is only produced for names that already
    // contain an underscore; the single form for names that do not.
    if underscored != name.contains('_') {
        return None;
    }
    Some(FortranSymbol {
        module: String::new(),
        name: name.to_string(),
    })
}

/// Demangles a Fortran symbol under the explicit-request policy: the
/// module-scoped forms plus the plain g77 `<name>_` form, which
/// auto-detection deliberately never claims.
///
/// The auto-detected path does not go through here — it calls
/// [`parse_module_symbol`] directly, so the plain form stays unreachable
/// without an explicit request. The mangling carries no type information, so
/// the options have no effect on the rendering.
pub(crate) fn demangle_explicit(symbol: &str, _opts: DemangleOptions) -> Option<String> {
    parse_module_symbol(symbol)
        .or_else(|| parse_plain_symbol(symbol))
        .map(|s| s.render())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn gfortran_module_procedures() {
        for (symbol, module, proc) in [
            ("__m_MOD_foo", "m", "foo"),
            ("m_MOD_foo", "m", "foo"),
            ("__my_module_MOD_my_proc", "my_module", "my_proc"),
            ("my_module_MOD_my_proc", "my_module", "my_proc"),
        ] {
            let parsed = parse_module_symbol(symbol).expect("module symbol parses");
            assert_eq!(parsed.module, module, "for {symbol}");
            assert_eq!(parsed.name, proc, "for {symbol}");
        }
    }

    #[test]
    fn gfortran_renamed_length_suffix() {
        let parsed = parse_module_symbol("__my_module_MOD_my_sub_12").expect("parses");
        assert_eq!(parsed.render(), "my_module::my_sub");
    }

    #[test]
    fn intel_mp_form() {
        let parsed = parse_module_symbol("my_module_mp_my_proc_").expect("parses");
        assert_eq!(parsed.render(), "my_module::my_proc");
        let parsed = parse_module_symbol("my_module_mp_my_proc").expect("parses");
        assert_eq!(parsed.render(), "my_module::my_proc");
    }

    #[test]
    fn plain_g77_form() {
        assert_eq!(
            parse_plain_symbol("init_").map(|s| s.render()).as_deref(),
            Some("init")
        );
        assert_eq!(
            parse_plain_symbol("my_sub__")
                .map(|s| s.render())
                .as_deref(),
            Some("my_sub")
        );
        // A single trailing underscore on an underscored name is not the
        // g77 convention; same for a doubled one on an underscore-free name.
        assert_eq!(parse_plain_symbol("my_sub_"), None);
        assert_eq!(parse_plain_symbol("init__"), None);
    }

    #[test]
    fn rejects_non_fortran() {
        assert_eq!(parse_module_symbol("libc.so.6"), None);
        assert_eq!(parse_module_symbol("main"), None);
        assert_eq!(parse_module_symbol("__libc_start_main"), None);
        assert_eq!(parse_module_symbol("_ZN3foo3barEv"), None);
        assert_eq!(parse_plain_symbol("_ZN3foo3barEv"), None);
        assert_eq!(parse_plain_symbol(""), None);
        assert_eq!(parse_plain_symbol("__"), None);
    }
}
