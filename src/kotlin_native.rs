//! Kotlin/Native symbol demangling.
//!
//! Kotlin Multiplatform binaries ship `.framework`/`.klib` objects whose
//! functions carry a mostly-readable encoding under the `_kfun:` prefix (a
//! leading underscore from the platform C-symbol convention, optionally
//! doubled by the linker). The qualified name is plain dotted Kotlin, and
//! parameter types are enclosed in a parenthesized list separated by `;`:
//!
//! ```text
//! _kfun:com.example.Foo.bar(kotlin.String;kotlin.Int)
//! _kfun:main(kotlin.Array<kotlin.String>)
//! _kfun:kotlin.io.println(kotlin.Any?)
//! ```
//!
//! This is closer to a pretty-printer than a classic demangler: the body is
//! split into its qualified name and parameter list, and the `kotlin.` prefix
//! of well-known standard-library types is elided so
//! `com.example.Foo.bar(kotlin.String;kotlin.Int)` renders as
//! `com.example.Foo.bar(String, Int)`. Bare names without a parameter list
//! (class initializers and top-level properties, e.g.
//! `_kfun:com.example.Counter`) pass through with the prefix stripped.
//!
//! References: [JetBrains/kotlin-native#755](https://github.com/JetBrains/kotlin-native/issues/755).

// Entry points are feature-gated; without the feature the module only
// provides detection predicates, and the rest is legitimately dead.
#![cfg_attr(not(feature = "kotlin-native"), allow(dead_code))]

use crate::DemangleOptions;

/// Strips the `kfun:` prefix (with any platform underscores) and returns the
/// body, or `None` when the symbol is not Kotlin/Native.
fn strip_prefix(symbol: &str) -> Option<&str> {
    symbol
        .strip_prefix("_kfun:")
        .or_else(|| symbol.strip_prefix("__kfun:"))
        .or_else(|| symbol.strip_prefix("kfun:"))
}

/// Whether a character ends the token preceding a type, so that what follows
/// starts a fresh type position.
fn opens_type_position(ch: char) -> bool {
    matches!(ch, '<' | '>' | ',' | '(' | ')' | ';' | ' ')
}

/// Elides the well-known standard-library package prefix of a type so
/// `kotlin.Array<kotlin.String?>` renders as `Array<String?>`. Nested
/// generics keep their structure.
///
/// Only a `kotlin.` that *begins a type* is elided — at the start of the
/// string or right after a separator. A `kotlin.` in any other position is
/// part of a longer package path (`com.mykotlin.Bar`, `com.x.kotlin.A`) and
/// is left alone, and finding one never stops the scan: every remaining type
/// position is still considered.
fn elide_kotlin_prefix(ty: &str) -> String {
    const PREFIX: &str = "kotlin.";

    let mut result = String::with_capacity(ty.len());
    let mut rest = ty;
    // The start of the string is a type position; afterwards it is whatever
    // the last copied character says.
    let mut at_type_start = true;
    while !rest.is_empty() {
        if at_type_start {
            if let Some(stripped) = rest.strip_prefix(PREFIX) {
                rest = stripped;
                continue;
            }
        }
        let ch = rest.chars().next().expect("rest is non-empty");
        result.push(ch);
        at_type_start = opens_type_position(ch);
        rest = &rest[ch.len_utf8()..];
    }
    result
}

/// A parsed Kotlin/Native symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KotlinNativeSymbol {
    /// Dotted qualified name (without parameters).
    pub qualified_name: String,
    /// Parameter type renderings, when the symbol carries a parameter list.
    pub parameters: Option<Vec<String>>,
    /// Return type rendering, when the symbol appends `:<type>` after the
    /// parameter list.
    pub return_type: Option<String>,
}

/// Parses a `kfun:` body into its qualified name and (optionally) parameter
/// and return types. The parameter list must close at the end of the body or
/// directly before a `:<return type>` suffix; anything else is left undemangled.
pub(crate) fn parse(symbol: &str) -> Option<KotlinNativeSymbol> {
    let body = strip_prefix(symbol)?;
    if body.is_empty() {
        return None;
    }

    // The qualified name ends at the first `(` that opens a trailing
    // parameter list. Kotlin identifiers cannot contain `(`, so no deeper
    // scan is needed to find it.
    let (qualified_name, signature) = match body.find('(') {
        Some(idx) => (&body[..idx], Some(&body[idx..])),
        None => (body, None),
    };
    if qualified_name.is_empty() {
        return None;
    }

    let mut parsed = KotlinNativeSymbol {
        qualified_name: qualified_name.to_string(),
        parameters: None,
        return_type: None,
    };

    if let Some(signature) = signature {
        // The parameter list must close at a balanced `)` which is either
        // the end of the symbol or directly followed by a `:<return type>`
        // suffix.
        let mut depth = 0usize;
        let mut close = None;
        for (offset, ch) in signature.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close?;
        let params_text = &signature[1..close];
        let after = &signature[close + 1..];
        // A `:Ret` return-type suffix may follow; the suffix must be the
        // last thing in the symbol.
        if !after.is_empty() {
            let ret = after.strip_prefix(':')?;
            if ret.is_empty() {
                return None;
            }
            parsed.return_type = Some(elide_kotlin_prefix(ret));
        }
        parsed.parameters = Some(if params_text.is_empty() {
            Vec::new()
        } else {
            params_text
                .split(';')
                .map(|p| elide_kotlin_prefix(p.trim()))
                .collect()
        });
    }

    Some(parsed)
}

/// Renders a parsed symbol for the given options.
pub(crate) fn render(parsed: &KotlinNativeSymbol, opts: DemangleOptions) -> String {
    let mut out = parsed.qualified_name.clone();
    if let Some(parameters) = &parsed.parameters {
        if opts.parameters {
            out.push('(');
            out.push_str(
                &parameters
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            out.push(')');
            if let Some(return_type) = &parsed.return_type {
                if opts.return_type {
                    out.push_str(": ");
                    out.push_str(return_type);
                }
            }
        }
    } else if let Some(return_type) = &parsed.return_type {
        if opts.return_type {
            out.push_str(": ");
            out.push_str(return_type);
        }
    }
    out
}

/// Demangles a Kotlin/Native symbol, or returns `None` when the symbol does
/// not carry the `kfun:` prefix.
pub(crate) fn demangle(symbol: &str, opts: DemangleOptions) -> Option<String> {
    parse(symbol).map(|parsed| render(&parsed, opts))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn qualified_name_with_parameters() {
        let parsed = parse("_kfun:com.example.Foo.bar(kotlin.String;kotlin.Int)").unwrap();
        assert_eq!(parsed.qualified_name, "com.example.Foo.bar");
        assert_eq!(
            parsed.parameters,
            Some(vec!["String".to_string(), "Int".to_string()])
        );
        assert_eq!(parsed.return_type, None);
    }

    #[test]
    fn generics_and_nullability_survive() {
        let parsed = parse("_kfun:main(kotlin.Array<kotlin.String>)").unwrap();
        assert_eq!(parsed.parameters, Some(vec!["Array<String>".to_string()]));
        let parsed = parse("_kfun:kotlin.io.println(kotlin.Any?)").unwrap();
        assert_eq!(parsed.parameters, Some(vec!["Any?".to_string()]));
        assert_eq!(parsed.qualified_name, "kotlin.io.println");
    }

    #[test]
    fn only_leading_kotlin_package_is_elided() {
        // `kotlin.` inside a longer package path is part of the name.
        assert_eq!(elide_kotlin_prefix("com.mykotlin.Bar"), "com.mykotlin.Bar");
        assert_eq!(elide_kotlin_prefix("com.x.kotlin.A"), "com.x.kotlin.A");
        // ...and hitting one does not stop later types from being elided.
        assert_eq!(
            elide_kotlin_prefix("kotlin.collections.Map<com.x.kotlin.A,kotlin.Int>"),
            "collections.Map<com.x.kotlin.A,Int>"
        );
        // Separators all open a fresh type position.
        assert_eq!(
            elide_kotlin_prefix("Pair<kotlin.Int, kotlin.String>"),
            "Pair<Int, String>"
        );
    }

    #[test]
    fn bare_names_pass_through() {
        let parsed = parse("_kfun:com.example.Counter").unwrap();
        assert_eq!(parsed.qualified_name, "com.example.Counter");
        assert_eq!(parsed.parameters, None);
    }

    #[test]
    fn return_type_suffix() {
        let parsed = parse("_kfun:com.example.Foo.size():kotlin.Int").unwrap();
        assert_eq!(parsed.parameters, Some(vec![]));
        assert_eq!(parsed.return_type.as_deref(), Some("Int"));
    }

    #[test]
    fn renders_with_options() {
        let opts = DemangleOptions::complete();
        assert_eq!(
            demangle("_kfun:com.example.Foo.bar(kotlin.String;kotlin.Int)", opts).as_deref(),
            Some("com.example.Foo.bar(String, Int)")
        );
        assert_eq!(
            demangle("_kfun:main(kotlin.Array<kotlin.String>)", opts).as_deref(),
            Some("main(Array<String>)")
        );
        assert_eq!(
            demangle(
                "_kfun:kotlin.io.println(kotlin.Any?)",
                DemangleOptions::name_only()
            )
            .as_deref(),
            Some("kotlin.io.println")
        );
        assert_eq!(
            demangle("_kfun:com.example.Foo.size():kotlin.Int", opts).as_deref(),
            Some("com.example.Foo.size(): Int")
        );
        assert_eq!(
            demangle(
                "_kfun:com.example.Foo.size():kotlin.Int",
                DemangleOptions::complete().return_type(false)
            )
            .as_deref(),
            Some("com.example.Foo.size()")
        );
    }

    #[test]
    fn rejects_non_kotlin() {
        assert_eq!(parse("_ZN3foo3barEv"), None);
        assert_eq!(parse("kfun"), None);
        assert_eq!(parse("_kfun:"), None);
        assert_eq!(parse("libc.so.6"), None);
        // Unbalanced or trailing parameter lists are not demangled.
        assert_eq!(parse("_kfun:foo(kotlin.Int"), None);
        assert_eq!(parse("_kfun:foo(kotlin.Int))"), None);
    }
}
