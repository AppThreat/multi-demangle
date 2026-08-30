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
//! | Intel ifort/ifx   | `<module>_mp_<proc>_`          | `<module>::<proc>`   |
//!
//! The procedure part is taken verbatim. gfortran appends no length or
//! disambiguation suffix: `subroutine interp_3` in `module numerics` emits
//! exactly `__numerics_MOD_interp_3`, so stripping a trailing `_<digits>`
//! would corrupt every procedure whose name ends in digits. Verified against
//! gfortran 12 via `contrib/` — see `contrib/fixtures/fortran/corpus.f90`.
//!
//! The plain g77 form `<name>_` is demangled only when explicitly requested
//! through [`crate::demangle_as`]: any C symbol may end in `_`, so
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
    if is_identifier(rest) {
        return Some(FortranSymbol {
            module: module.to_ascii_lowercase(),
            name: rest.to_string(),
        });
    }
    None
}

/// Parses the plain g77 form: `<name>_` (or `<name>__` for names that already
/// contain an underscore), where the trailing underscore(s) are appended by
/// the compiler and not part of the source name.
///
/// Deliberately not part of auto-detection — a C symbol may end in `_` — so
/// this form is only demangled on explicit request (see [`crate::demangle_as`]).
pub(crate) fn parse_plain_symbol(symbol: &str) -> Option<FortranSymbol> {
    let base = symbol.strip_suffix('_')?;
    // g77/f2c (and gfortran under `-fsecond-underscore`) append a *second*
    // underscore to names that already contain one. gfortran does not do
    // that by default — `subroutine two_words` really does emit
    // `two_words_` — so a single trailing underscore is the common case and
    // the doubled one is only unwrapped when the remaining name is itself
    // underscored. Otherwise `init__` is the mangling of a subroutine
    // genuinely named `init_`.
    let name = match base.strip_suffix('_') {
        Some(inner) if inner.contains('_') => inner,
        _ => base,
    };
    if name.is_empty() || !is_identifier(name) {
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

    /// Procedure names ending in `_<digits>` keep those digits. These four
    /// symbols are verbatim gfortran 12 output for the procedures of the
    /// same name in `contrib/fixtures/fortran/corpus.f90`.
    #[test]
    fn procedure_names_ending_in_digits_are_preserved() {
        for (symbol, expected) in [
            ("__numerics_MOD_interp_3", "numerics::interp_3"),
            ("__numerics_MOD_step_12", "numerics::step_12"),
            ("__numerics_MOD_solve_2d", "numerics::solve_2d"),
            ("__numerics_MOD_plain", "numerics::plain"),
        ] {
            let parsed = parse_module_symbol(symbol).expect("module symbol parses");
            assert_eq!(parsed.render(), expected, "for {symbol}");
        }
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
        // gfortran appends a single underscore even to names that already
        // contain one — `standalone_` and `two_words_` are verbatim
        // gfortran 12 output for the fixture's bare subprograms, so a single
        // trailing underscore on an underscored name must be accepted.
        assert_eq!(
            parse_plain_symbol("standalone_")
                .map(|s| s.render())
                .as_deref(),
            Some("standalone")
        );
        assert_eq!(
            parse_plain_symbol("two_words_")
                .map(|s| s.render())
                .as_deref(),
            Some("two_words")
        );
        // `init__` is not the doubled form (the inner name has no
        // underscore): it is a subprogram genuinely named `init_`.
        assert_eq!(
            parse_plain_symbol("init__").map(|s| s.render()).as_deref(),
            Some("init_")
        );
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
