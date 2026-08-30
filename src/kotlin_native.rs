//! Kotlin/Native symbol demangling.
//!
//! # Scope (verified against the compiler)
//!
//! The readable `kfun:` spelling this module parses is still what current
//! Kotlin/Native emits: compiling a fixture with the 2.0.21 prebuilt
//! compiler (see `contrib/docker/kotlin-native.Dockerfile`) puts ~950
//! `kfun:` symbols in a 60-line program's object files. The backend is
//! therefore live, not a historical dialect — but the spelling modern
//! compilers emit differs from the dotted form
//! (`kfun:com.example.Foo.bar(kotlin.String;kotlin.Int)`) that the 2018
//! GitHub issue this module used as its reference shows. The actual grammar:
//!
//! ```text
//! kfun:<path>[(<params>)][{<type params>}][<return type>][-trampoline]
//! _kfun:com.example#describe(kotlin.Int;kotlin.String){}kotlin.String
//! _kfun:com.example.Counter#increment(kotlin.Int){}kotlin.Int
//! _kfun:com.example#main(kotlin.Array<kotlin.String>){}
//! _kfun:com.example.Color#$getEnumAt#static(kotlin.Int){}com.example.Color
//! _kfun:com.example.Rect#area(){}kotlin.Double-trampoline
//! ```
//!
//! The package path is dotted, `#` separates the container from the member
//! name, a trailing `#static`/`#internal` segment is a declaration marker
//! rather than a name (optionally carrying a numeric disambiguator, as in
//! `<get-rangeLength>#internal.14`), parameters are `;`-separated inside parentheses, the braced
//! block lists generic type-parameter bounds (`0§<kotlin.Any?>`, usually
//! empty), and a `-trampoline` suffix marks compiler-generated dispatch
//! thunks. Compiler-generated accessor names (`<init>`, `<get-x>`,
//! `<set-x>`) and escaped characters in names (`shout__at__kotlin.String`)
//! pass through verbatim. A legacy form with a `:<return type>` suffix
//! instead of the braced block is accepted as well.
//!
//! This is closer to a pretty-printer than a classic demangler. Rendering
//! choices, documented here because there is no reference demangler to
//! match: `#` renders as `.` (`com.example.Counter.increment`), the
//! `kotlin.` prefix of well-known standard-library types is elided so
//! `kotlin.String` renders as `String`, the type-parameter block is parsed
//! but dropped (it carries bounds, not names), and the `static`/`internal`
//! declaration markers and the trampoline marker render as trailing tags
//! (` [static]`, ` [trampoline]`) rather than as name segments — they
//! describe the declaration, not its path, and appending them to the name
//! would make the marker the leaf. The compiler's `$<bridge-…>` thunks repeat their signature
//! after the return type; that tail renders verbatim as part of the return
//! type rather than being reparsed — they are compiler glue, and the
//! rendering stays stable and recorded in the corpus snapshots.

// Entry points are feature-gated; without the feature the module only
// provides detection predicates, and the rest is legitimately dead.
#![cfg_attr(not(feature = "kotlin-native"), allow(dead_code))]

use crate::DemangleOptions;

/// Compiler markers that appear as trailing `#`-separated segments on the
/// qualified name. They describe the declaration, not its path, so they are
/// stripped before the name is rendered rather than becoming name segments.
const MARKERS: [&str; 2] = ["static", "internal"];

/// Whether a trailing `#`-separated segment is a compiler marker rather than
/// a member name. A marker may carry a numeric disambiguator
/// (`<get-rangeLength>#internal.14`), which is kept: it is what distinguishes
/// two otherwise identical symbols.
///
/// `internal` and `static` cannot be bare Kotlin identifiers, so a trailing
/// segment spelled either way is always a marker, never a member.
fn is_marker(segment: &str) -> bool {
    let base = match segment.rsplit_once('.') {
        Some((base, suffix))
            if !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()) =>
        {
            base
        }
        _ => segment,
    };
    MARKERS.contains(&base)
}

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
    /// Dotted qualified name (without parameters). `#` separators have been
    /// rendered as `.`.
    pub qualified_name: String,
    /// Parameter type renderings, when the symbol carries a parameter list.
    pub parameters: Option<Vec<String>>,
    /// Generic type-parameter bounds from the braced block (`0§<kotlin.Any?>`
    /// entries), when present. Parsed but not rendered: the block carries
    /// bounds, not the (erased) parameter names.
    pub type_parameters: Option<Vec<String>>,
    /// Return type rendering, when the symbol carries one after the
    /// parameter list.
    pub return_type: Option<String>,
    /// Compiler markers carried as trailing `#` segments (`static`,
    /// `internal`), in the order they appeared. Kept out of the qualified
    /// name: they describe the declaration, not its path.
    pub markers: Vec<String>,
    /// Whether the `-trampoline` thunk marker was present.
    pub trampoline: bool,
}

/// Parses a `kfun:` body into its qualified name and optional parameter
/// list, type-parameter block, and return type.
///
/// After a balanced parameter list the body must end, or carry a braced
/// type-parameter block (optionally followed by a return type), or a legacy
/// `:<return type>` suffix — anything else is left undemangled.
pub(crate) fn parse(symbol: &str) -> Option<KotlinNativeSymbol> {
    let body = strip_prefix(symbol)?;
    let (body, trampoline) = match body.strip_suffix("-trampoline") {
        Some(stripped) => (stripped, true),
        None => (body, false),
    };
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

    // Trailing `#static` / `#internal` segments are markers, not path
    // components. Peeling them off before the `#` -> `.` rewrite keeps the
    // leaf name correct: `Color#$getEnumAt#static` names `$getEnumAt`, and
    // leaving the marker attached made it the leaf for every consumer that
    // splits the qualified name on `.`.
    let mut qualified_name = qualified_name;
    let mut markers = Vec::new();
    while let Some((head, last)) = qualified_name.rsplit_once('#') {
        if !is_marker(last) {
            break;
        }
        // A marker is never the whole symbol; `#static` alone has no name.
        if head.is_empty() {
            return None;
        }
        markers.push(last.to_string());
        qualified_name = head;
    }
    markers.reverse();

    let mut parsed = KotlinNativeSymbol {
        // `#` separates the container from the member name; it renders as
        // the plain path separator.
        qualified_name: qualified_name.replace('#', "."),
        parameters: None,
        type_parameters: None,
        return_type: None,
        markers,
        trampoline,
    };

    if let Some(signature) = signature {
        // The parameter list must close at a balanced `)`.
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
        parsed.parameters = Some(if params_text.is_empty() {
            Vec::new()
        } else {
            params_text
                .split(';')
                .map(|p| elide_kotlin_prefix(p.trim()))
                .collect()
        });

        let after = &signature[close + 1..];
        if after.is_empty() {
            return Some(parsed);
        }
        if let Some(ret) = after.strip_prefix(':') {
            // Legacy return-type suffix.
            if ret.is_empty() {
                return None;
            }
            parsed.return_type = Some(elide_kotlin_prefix(ret));
            return Some(parsed);
        }
        // Modern form: a braced type-parameter block, optionally followed by
        // the return type.
        let after = after.strip_prefix('{')?;
        let block_end = after.find('}')?;
        let block = &after[..block_end];
        if !block.is_empty() {
            parsed.type_parameters = Some(block.split(';').map(str::to_string).collect());
        }
        let ret = after[block_end + 1..]
            .strip_prefix(':')
            .unwrap_or(&after[block_end + 1..]);
        if !ret.is_empty() {
            parsed.return_type = Some(elide_kotlin_prefix(ret));
        }
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
    if opts.parameters {
        // Declaration markers and the thunk marker trail the signature. Like
        // the parameters they are not part of the name, so they drop from
        // name-only renderings — but they must stay visible by default, or a
        // trampoline aliases the function it dispatches to.
        for marker in &parsed.markers {
            out.push_str(" [");
            out.push_str(marker);
            out.push(']');
        }
        if parsed.trampoline {
            out.push_str(" [trampoline]");
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
    fn modern_member_function() {
        let parsed = parse("_kfun:com.example.Counter#increment(kotlin.Int){}kotlin.Int").unwrap();
        assert_eq!(parsed.qualified_name, "com.example.Counter.increment");
        assert_eq!(parsed.parameters, Some(vec!["Int".to_string()]));
        assert_eq!(parsed.type_parameters, None);
        assert_eq!(parsed.return_type.as_deref(), Some("Int"));
        assert!(!parsed.trampoline);
        assert_eq!(
            demangle(
                "_kfun:com.example.Counter#increment(kotlin.Int){}kotlin.Int",
                DemangleOptions::complete()
            )
            .as_deref(),
            Some("com.example.Counter.increment(Int): Int")
        );
    }

    #[test]
    fn top_level_function_and_empty_signature_parts() {
        let parsed =
            parse("_kfun:com.example#describe(kotlin.Int;kotlin.String){}kotlin.String").unwrap();
        assert_eq!(parsed.qualified_name, "com.example.describe");
        assert_eq!(
            parsed.parameters,
            Some(vec!["Int".to_string(), "String".to_string()])
        );
        // An empty braced block carries no type parameters.
        assert_eq!(parsed.type_parameters, None);
        // A function with no return value (`{}` and nothing after).
        let parsed = parse("_kfun:com.example#main(kotlin.Array<kotlin.String>){}").unwrap();
        assert_eq!(parsed.qualified_name, "com.example.main");
        assert_eq!(parsed.return_type, None);
        assert_eq!(parsed.parameters, Some(vec!["Array<String>".to_string()]));
    }

    /// `#static` / `#internal` are declaration markers, not path components:
    /// they must not become the leaf name. All four symbols here are verbatim
    /// from `tests/corpus/kotlin_symbols.txt` (Kotlin/Native 2.0.21).
    #[test]
    fn markers_are_not_name_segments() {
        let parsed =
            parse("_kfun:com.example.Color#$getEnumAt#static(kotlin.Int){}com.example.Color")
                .unwrap();
        assert_eq!(parsed.qualified_name, "com.example.Color.$getEnumAt");
        assert_eq!(parsed.markers, ["static"]);
        assert_eq!(parsed.return_type.as_deref(), Some("com.example.Color"));

        // The marker can also trail a symbol with no signature at all.
        let parsed = parse("_kfun:com.example.Color.$init_global#internal").unwrap();
        assert_eq!(parsed.qualified_name, "com.example.Color.$init_global");
        assert_eq!(parsed.markers, ["internal"]);
        assert_eq!(parsed.parameters, None);

        let parsed = parse("_kfun:com.example.Counter.<set-value>#internal").unwrap();
        assert_eq!(parsed.qualified_name, "com.example.Counter.<set-value>");
        assert_eq!(parsed.markers, ["internal"]);

        // Markers render as trailing tags, and drop from name-only output.
        let sym =
            "_kfun:com.example.Counter#<get-$companion>#static(){}com.example.Counter.Companion";
        assert_eq!(
            demangle(sym, DemangleOptions::complete()).as_deref(),
            Some("com.example.Counter.<get-$companion>(): com.example.Counter.Companion [static]")
        );
        assert_eq!(
            demangle(sym, DemangleOptions::name_only()).as_deref(),
            Some("com.example.Counter.<get-$companion>")
        );

        // A marker may carry a numeric disambiguator, which is kept: it is
        // the only thing separating this symbol from another with the same
        // name.
        let parsed = parse("_kfun:kotlin.text.<get-rangeLength>#internal.14").unwrap();
        assert_eq!(parsed.qualified_name, "kotlin.text.<get-rangeLength>");
        assert_eq!(parsed.markers, ["internal.14"]);

        // A marker with no name in front of it is not a symbol.
        assert_eq!(parse("_kfun:#static"), None);
    }

    #[test]
    fn accessor_and_extension_names_pass_through() {
        let parsed = parse("_kfun:com.example.Point#<get-x>(){}kotlin.Int").unwrap();
        assert_eq!(parsed.qualified_name, "com.example.Point.<get-x>");
        // Extension receivers and other escaped spellings are part of the
        // name and render verbatim.
        let parsed = parse("_kfun:com.example#shout__at__kotlin.String(){}kotlin.String").unwrap();
        assert_eq!(
            parsed.qualified_name,
            "com.example.shout__at__kotlin.String"
        );
    }

    #[test]
    fn generic_type_parameter_block() {
        let parsed = parse("_kfun:com.example#genericIdentity(0:0){0§<kotlin.Any?>}0:0").unwrap();
        assert_eq!(parsed.parameters, Some(vec!["0:0".to_string()]));
        assert_eq!(
            parsed.type_parameters,
            Some(vec!["0§<kotlin.Any?>".to_string()])
        );
        assert_eq!(parsed.return_type.as_deref(), Some("0:0"));
        // Multiple bounds, `;`-separated.
        let parsed = parse(
            "_kfun:kotlin.text#joinTo(0:0;1:0){0§<kotlin.Any?>;1§<kotlin.text.Appendable>}1:0",
        )
        .unwrap();
        assert_eq!(
            parsed.type_parameters,
            Some(vec![
                "0§<kotlin.Any?>".to_string(),
                "1§<kotlin.text.Appendable>".to_string()
            ])
        );
    }

    #[test]
    fn trampoline_suffix() {
        let parsed = parse("_kfun:com.example.Rect#area(){}kotlin.Double-trampoline").unwrap();
        assert!(parsed.trampoline);
        assert_eq!(parsed.return_type.as_deref(), Some("Double"));
        assert_eq!(
            demangle(
                "_kfun:com.example.Rect#area(){}kotlin.Double-trampoline",
                DemangleOptions::complete()
            )
            .as_deref(),
            Some("com.example.Rect.area(): Double [trampoline]")
        );
    }

    #[test]
    fn legacy_return_type_suffix() {
        let parsed = parse("_kfun:com.example.Foo.size():kotlin.Int").unwrap();
        assert_eq!(parsed.parameters, Some(vec![]));
        assert_eq!(parsed.return_type.as_deref(), Some("Int"));
        // Unbalanced or trailing parameter lists are not demangled.
        assert_eq!(parse("_kfun:foo(kotlin.Int"), None);
        assert_eq!(parse("_kfun:foo(kotlin.Int))"), None);
        assert_eq!(parse("_kfun:foo(kotlin.Int)junk"), None);
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
    fn renders_with_options() {
        let opts = DemangleOptions::complete();
        assert_eq!(
            demangle(
                "_kfun:com.example#describe(kotlin.Int;kotlin.String){}kotlin.String",
                opts
            )
            .as_deref(),
            Some("com.example.describe(Int, String): String")
        );
        assert_eq!(
            demangle(
                "_kfun:kotlin.io.println(kotlin.Any?)",
                DemangleOptions::name_only()
            )
            .as_deref(),
            Some("kotlin.io.println")
        );
        // Name-only drops the whole signature, trampoline included.
        assert_eq!(
            demangle(
                "_kfun:com.example.Rect#area(){}kotlin.Double-trampoline",
                DemangleOptions::name_only()
            )
            .as_deref(),
            Some("com.example.Rect.area")
        );
    }

    #[test]
    fn rejects_non_kotlin() {
        assert_eq!(parse("_ZN3foo3barEv"), None);
        assert_eq!(parse("kfun"), None);
        assert_eq!(parse("_kfun:"), None);
        assert_eq!(parse("libc.so.6"), None);
    }
}
