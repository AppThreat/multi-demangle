//! Ada (GNAT) symbol demangling.
//!
//! GNAT flattens Ada's hierarchical names into `pkg__child__subprogram`
//! symbols with a set of small encodings on top: the `_ada_` prefix marks
//! library-level subprograms, `__<digits>` disambiguates body/spec and
//! overload numbers, package elaboration procedures are leaves named
//! `_elabb`/`_elabs` (rendered `pkg'Elab_Body`/`pkg'Elab_Spec`), task and
//! record types carry generated companions (`TB` task body, `IP`
//! initialization procedure, `E`/`Z` elaboration and size variables), `B`
//! markers encode task bodies, `B_<digits>` components are anonymous
//! blocks, `U<hex>`/`W<hex>` escape non-lowercase characters, and operator
//! subprograms are named `O<name>`.
//!
//! The encoding is documented in GCC's `ada/exp_dbug.ads` ("Encoding of
//! Identifiers"); the compiler-generated task companions that the `.ads`
//! only implies are built in `exp_ch9.adb` (`TB` body procedure, `E`
//! elaboration variable, `Z` storage-size variable, `V` corresponding
//! record) and `exp_ch3.adb` (`IP` initialization procedure). The structure
//! of this implementation follows the MIT-licensed `ada-demangle` crate by
//! Pernosco (<https://github.com/Pernosco/ada-demangle>), with operator
//! rendering (quoted, as `pkg."="`) matching the GNU reference demangler.
//!
//! # Auto-detection
//!
//! Ada is **explicit-request-only**. GNAT's `pkg__sub` encoding has no
//! reserved prefix — it is a flat identifier with a separator, which is also
//! how a great many C projects spell an internal function (`_uv__io_close`,
//! `_thread_db___pthread_keys`), and on Mach-O every symbol picks up a
//! leading underscore, so the C function `ada_copy` is spelled exactly like
//! GNAT's library-level subprogram `Copy`. Since C symbols outnumber Ada ones
//! by orders of magnitude in any real symbol table, guessing by shape
//! rewrites far more correct names than it fixes. Ada is therefore reached
//! only by naming the language.
//!
//! # Examples
//!
//! ```
//! # #[cfg(feature = "ada")] {
//! use multi_demangle::{demangle_as, DemangleOptions};
//!
//! assert_eq!(
//!     demangle_as("ada", "ada__exceptions__last_chance_handlerXn", DemangleOptions::complete()),
//!     Some("ada.exceptions.last_chance_handler".to_string())
//! );
//! # }
//! ```

// Entry points are feature-gated; without the feature the module only
// provides detection predicates, and the rest is legitimately dead.
#![cfg_attr(not(feature = "ada"), allow(dead_code))]

use crate::DemangleOptions;

/// A parsed GNAT symbol.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AdaSymbol {
    /// Package/component path, outermost first.
    pub namespace: Vec<String>,
    /// Leaf name; operators render quoted (`"="`), which is also what
    /// distinguishes them.
    pub name: String,
    /// The package elaboration procedure (`_elabb`/`_elabs` leaf), rendered
    /// with attribute syntax (`corpus'Elab_Body`) instead of a dot path.
    pub elab: Option<&'static str>,
    /// Whether the `B` marker marked this as a task body. Not recoverable
    /// from the rendered name, unlike the operator/plain distinction.
    pub task_body: bool,
}

impl AdaSymbol {
    /// Renders the symbol the way Ada source spells it: dot-separated path
    /// with the leaf appended. Elaboration procedures render with attribute
    /// syntax, matching the GNU reference demangler.
    pub fn render(&self) -> String {
        let mut out = self.namespace.join(".");
        if let Some(elab) = self.elab {
            if !out.is_empty() {
                out.push('\'');
            }
            out.push_str(elab);
            return out;
        }
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(&self.name);
        out
    }
}

/// Whether `bytes` is a plain GNAT identifier character.
fn is_plain(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
}

/// Splits at the first `__`, returning the part before it and the rest.
fn split_prefix(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    bytes
        .windows(2)
        .position(|w| w == b"__")
        .map(|idx| (&bytes[..idx], &bytes[idx + 2..]))
}

/// Splits off a trailing `__<suffix>`, returning the part before it and the
/// suffix.
fn split_suffix(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    (0..bytes.len().saturating_sub(1))
        .rev()
        .find(|&idx| &bytes[idx..idx + 2] == b"__")
        .map(|idx| (&bytes[..idx], &bytes[idx + 2..]))
}

/// Decodes a run of lowercase hex digits (2 for `U`, 4 for `W`).
fn hex_to_u16(bytes: &[u8]) -> Option<u16> {
    if bytes.is_empty() || bytes.len() > 4 {
        return None;
    }
    let mut val = 0u16;
    for b in bytes {
        val = (val << 4)
            + match b {
                b'0'..=b'9' => u16::from(b - b'0'),
                b'a'..=b'f' => u16::from(b - b'a' + 10),
                _ => return None,
            };
    }
    Some(val)
}

/// Decodes a GNAT identifier: `U<hex2>` and `W<hex4>` escapes stand for
/// arbitrary characters; everything else is literal.
fn decode_identifier(bytes: &[u8]) -> Option<String> {
    if bytes.iter().all(|&b| is_plain(b)) {
        return Some(String::from_utf8_lossy(bytes).into_owned());
    }
    // GNAT lowercases identifiers; uppercase bytes must be `U`/`W` escapes.
    let mut buf: Vec<u16> = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while !rest.is_empty() {
        match rest[0] {
            // Uppercase letters only occur as escape prefixes.
            b'U' => {
                if rest.len() < 3 {
                    return None;
                }
                buf.push(hex_to_u16(&rest[1..3])?);
                rest = &rest[3..];
            }
            b'W' => {
                if rest.len() < 5 {
                    return None;
                }
                buf.push(hex_to_u16(&rest[1..5])?);
                rest = &rest[5..];
            }
            b if is_plain(b) => {
                buf.push(u16::from(b));
                rest = &rest[1..];
            }
            _ => return None,
        }
    }
    String::from_utf16(&buf).ok()
}

/// The GNAT operator table: `O<name>` subprograms render as the operator.
fn operator_name(bytes: &[u8]) -> Option<&'static str> {
    let body = bytes.strip_prefix(b"O")?;
    Some(match body {
        b"abs" => "abs",
        b"and" => "and",
        b"mod" => "mod",
        b"not" => "not",
        b"or" => "or",
        b"rem" => "rem",
        b"xor" => "xor",
        b"eq" => "=",
        b"ne" => "/=",
        b"lt" => "<",
        b"le" => "<=",
        b"gt" => ">",
        b"ge" => ">=",
        b"add" => "+",
        b"sub" | b"subtract" => "-",
        b"concat" => "&",
        b"mul" | b"multiply" => "*",
        b"div" | b"divide" => "/",
        b"exp" | b"expon" => "**",
        _ => return None,
    })
}

/// Whether the component is an anonymous block (`B_<digits>`).
fn is_anonymous_block(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes.starts_with(b"B_") && bytes[2..].iter().all(u8::is_ascii_digit)
}

/// Whether the symbol's character set fits GNAT mangling.
fn plausible_charset(symbol: &str) -> bool {
    symbol
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Strips GCC's `.<digits>` local-symbol suffix, which it appends to
/// function-local entities that were promoted to file scope — GNAT emits
/// `corpus__compute__outer_block__inner.0` for a procedure nested inside a
/// named block. The suffix is a linker disambiguator, not part of the Ada
/// name, and `c++filt -s gnat` drops it the same way.
fn strip_local_suffix(symbol: &str) -> &str {
    match symbol.rsplit_once('.') {
        Some((base, digits))
            if !base.is_empty()
                && !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit()) =>
        {
            base
        }
        _ => symbol,
    }
}

/// Cheap detection predicate: GNAT names are ASCII identifiers carrying a
/// `__` separator (or the `_ada_` prefix, or an operator name). The demangler
/// below does the strict validation.
pub(crate) fn is_maybe_ada(symbol: &str) -> bool {
    let symbol = strip_local_suffix(symbol);
    if !plausible_charset(symbol) || symbol.len() < 3 {
        return false;
    }
    let first = symbol.as_bytes()[0];
    if !(first.is_ascii_lowercase() || first == b'_' || first == b'O') {
        return false;
    }
    if symbol.starts_with("_ada_") && symbol.len() > 5 {
        return true;
    }
    symbol.contains("__") || operator_name(symbol.as_bytes()).is_some()
}

/// Parses a GNAT symbol, or returns `None` when it does not validate.
pub(crate) fn parse(symbol: &str) -> Option<AdaSymbol> {
    let bytes = strip_local_suffix(symbol).as_bytes();

    // Library-level subprograms carry the `_ada_` prefix.
    let bytes = bytes.strip_prefix(b"_ada_").unwrap_or(bytes);
    if bytes.is_empty() {
        return None;
    }

    // A trailing `__<digits>` is a body/spec disambiguation or overload
    // number; drop it. The suffix must be *entirely* digits — `pkg__proc__2a`
    // is not an overload number, and dropping it would silently lose the
    // component.
    let bytes = match split_suffix(bytes) {
        Some((base, suffix)) if !suffix.is_empty() && suffix.iter().all(u8::is_ascii_digit) => base,
        _ => bytes,
    };

    // Walk the package components.
    let mut namespace = Vec::new();
    let mut rest = bytes;
    while let Some((prefix, remainder)) = split_prefix(rest) {
        rest = remainder;
        // A `T` in a component marks a task/entry suffix; everything from
        // the last `T` on belongs to it.
        let prefix = match prefix.iter().rposition(|&b| b == b'T') {
            Some(idx) => &prefix[..idx],
            None => prefix,
        };
        // Anonymous blocks carry no name.
        if is_anonymous_block(prefix) {
            continue;
        }
        if prefix.is_empty() {
            return None;
        }
        namespace.push(decode_identifier(prefix)?);
    }

    // Package elaboration procedures are leaves named `_elabb`/`_elabs`
    // (exp_dbug.ads). They only attach to a package path — a bare `__elabb`
    // has nothing to elaborate — and render with attribute syntax.
    let elab = if rest == b"_elabb" && !namespace.is_empty() {
        Some("Elab_Body")
    } else if rest == b"_elabs" && !namespace.is_empty() {
        Some("Elab_Spec")
    } else {
        None
    };

    // Compiler-generated task companions (exp_ch9.adb): the task body
    // procedure `<task>TB` for an explicit task type (`<task>TKB` for the
    // `TK`-qualified spelling newer compilers document), and the
    // initialization procedure `<type>IP` (exp_ch3.adb), which for a
    // task/protected type hangs off its corresponding record `<type>V`
    // (exp_dbug.ads). Uppercase never occurs in plain identifiers, so these
    // suffixes can only be markers.
    let mut task_body = false;
    if let Some(base) = rest
        .strip_suffix(&b"TKB"[..])
        .or_else(|| rest.strip_suffix(&b"TB"[..]))
    {
        if base.is_empty() {
            return None;
        }
        task_body = true;
        rest = base;
    } else if let Some(init_base) = rest.strip_suffix(&b"IP"[..]) {
        if init_base.is_empty() {
            return None;
        }
        rest = init_base.strip_suffix(b"V").unwrap_or(init_base);
        if rest.is_empty() {
            return None;
        }
    }

    // The remaining leaf may carry a trailing calling-convention/task marker
    // (`X`/`N`/`E`/`Z`/`B`: convention suffixes, the task/unit elaboration
    // flag `_E`, the task storage-size variable `Z`, anonymous task bodies
    // `B`); uppercase only occurs in markers and escapes, so scanning for
    // the last marker byte is safe. Some markers are joined by an
    // underscore (`corpus__child_E`), so a trailing separator after the
    // strip is part of the marker: Ada identifiers cannot end in `_`.
    let (leaf_bytes, marker_task_body) = match rest
        .iter()
        .rposition(|b| matches!(b, b'X' | b'N' | b'E' | b'Z' | b'B'))
    {
        Some(idx) => {
            let mut leaf = &rest[..idx];
            while leaf.last() == Some(&b'_') {
                leaf = &leaf[..leaf.len() - 1];
            }
            (leaf, rest[idx] == b'B')
        }
        None => (rest, false),
    };
    let task_body = task_body || marker_task_body;

    if let Some(elab) = elab {
        // An elaboration leaf carries no additional name; the full leaf was
        // the `_elabb`/`_elabs` marker itself.
        return Some(AdaSymbol {
            namespace,
            name: String::new(),
            elab: Some(elab),
            task_body,
        });
    }
    if leaf_bytes.is_empty() {
        return None;
    }

    let name = match operator_name(leaf_bytes) {
        Some(op) => format!("\"{op}\""),
        None => decode_identifier(leaf_bytes)?,
    };
    Some(AdaSymbol {
        namespace,
        name,
        elab: None,
        task_body,
    })
}

/// Demangles an Ada symbol with the given options (the mangling carries no
/// parameter or return type information, so the options have no effect).
pub(crate) fn demangle(symbol: &str, _opts: DemangleOptions) -> Option<String> {
    if !is_maybe_ada(symbol) {
        return None;
    }
    Some(parse(symbol)?.render())
}

#[cfg(test)]
mod test {
    use super::*;

    fn dem(sym: &str) -> String {
        demangle(sym, DemangleOptions::complete()).expect("demangles")
    }

    #[test]
    fn package_paths() {
        assert_eq!(
            dem("ada__exceptions__last_chance_handlerXn"),
            "ada.exceptions.last_chance_handler"
        );
        assert_eq!(
            dem("system__storage_elements__s_stalib_adafinal"),
            "system.storage_elements.s_stalib_adafinal"
        );
        // Body/overload suffixes are dropped.
        assert_eq!(dem("module__square__2"), "module.square");
        // ...but only when they are entirely digits; otherwise the trailing
        // component is a real name and must survive.
        assert_eq!(dem("module__square__2a"), "module.square.2a");
    }

    #[test]
    fn library_level_prefix() {
        assert_eq!(dem("_ada_main"), "main");
        assert_eq!(dem("_ada_ada__initialization"), "ada.initialization");
    }

    #[test]
    fn identifiers_with_escapes() {
        // `U41` encodes the character 'A'.
        assert_eq!(dem("module__U41bc__proc"), "module.Abc.proc");
        // `W0041` encodes the code point U+0041.
        assert_eq!(dem("module__W0041bc__proc"), "module.Abc.proc");
    }

    #[test]
    fn operators() {
        assert_eq!(dem("Oeq"), "\"=\"");
        assert_eq!(dem("module__Oadd"), "module.\"+\"");
        assert_eq!(dem("module__Ole"), "module.\"<=\"");
    }

    /// GNAT output for `contrib/fixtures/ada/corpus.adb`, cross-checked
    /// against `c++filt -s gnat`.
    #[test]
    fn gcc_local_symbol_suffix() {
        assert_eq!(
            dem("corpus__compute__outer_block__inner.0"),
            "corpus.compute.outer_block.inner"
        );
        // The suffix is only stripped when it is entirely digits.
        assert_eq!(
            demangle("corpus__proc.a", DemangleOptions::complete()),
            None
        );
    }

    #[test]
    fn anonymous_blocks() {
        assert_eq!(
            dem("ada_main__finalize_library__B_4__reraise_library_exception_if_any"),
            "ada_main.finalize_library.reraise_library_exception_if_any"
        );
    }

    #[test]
    fn task_bodies() {
        let parsed = parse("module__my_taskB").expect("parses");
        assert!(parsed.task_body);
        assert_eq!(parsed.namespace, ["module"]);
        assert_eq!(parsed.name, "my_task");

        // Explicit task types name their body procedure `<task>TB`
        // (exp_ch9.adb, `Build_Task_Proc_Specification`); newer compilers
        // document the `TK`-qualified spelling. `c++filt -s gnat` has no
        // handling for either — this rendering is ours, per exp_dbug.ads.
        assert_eq!(dem("corpus__workerTB"), "corpus.worker");
        assert_eq!(dem("p__taskobjTKB"), "p.taskobj");
        let parsed = parse("corpus__workerTB").expect("parses");
        assert!(parsed.task_body);
    }

    /// Package elaboration procedures: the `_elabb`/`_elabs` leaves render
    /// with attribute syntax, matching `c++filt -s gnat` (`corpus___elabb`
    /// -> `corpus'Elab_Body`). A plain `elabb` leaf without the leading
    /// underscore is an ordinary identifier, as in the reference demangler.
    #[test]
    fn elaboration_procedures() {
        assert_eq!(dem("corpus___elabb"), "corpus'Elab_Body");
        assert_eq!(dem("corpus___elabs"), "corpus'Elab_Spec");
        assert_eq!(dem("a__b___elabb"), "a.b'Elab_Body");
        assert_eq!(dem("corpus__elabb"), "corpus.elabb");
        // Nothing to elaborate.
        assert_eq!(demangle("__elabb", DemangleOptions::complete()), None);
    }

    /// Compiler-generated task companions: `E` (elaboration flag) and `Z`
    /// (storage-size variable) strip as markers; `IP` is the initialization
    /// procedure of a type, which for a task type hangs off its
    /// corresponding record `V` (exp_ch9.adb / exp_ch3.adb). The reference
    /// demangler fails on all of these; the renderings follow exp_dbug.ads.
    #[test]
    fn task_companions() {
        assert_eq!(dem("corpus__workerE"), "corpus.worker");
        assert_eq!(dem("corpus__workerZ"), "corpus.worker");
        assert_eq!(dem("corpus__valueIP"), "corpus.value");
        assert_eq!(dem("corpus__workerVIP"), "corpus.worker");
        // A single-letter marker with no leaf underneath stays rejected.
        assert_eq!(demangle("corpus_E", DemangleOptions::complete()), None);
    }

    #[test]
    fn rejects_non_ada() {
        assert_eq!(demangle("libc.so.6", DemangleOptions::complete()), None);
        assert_eq!(demangle("_ZN3foo3barEv", DemangleOptions::complete()), None);
        assert_eq!(demangle("main", DemangleOptions::complete()), None);
        assert_eq!(
            demangle("__libc_start_main", DemangleOptions::complete()),
            None
        );
        assert_eq!(demangle("_ada_", DemangleOptions::complete()), None);
        assert_eq!(demangle("Ounknown", DemangleOptions::complete()), None);
        // `B53b` from the reference test suite has an empty leaf.
        assert_eq!(demangle("B53b", DemangleOptions::complete()), None);
    }

    #[test]
    fn detection_predicate() {
        assert!(is_maybe_ada("ada__exceptions__last_chance_handlerXn"));
        assert!(is_maybe_ada("_ada_main"));
        assert!(is_maybe_ada("Oeq"));
        assert!(!is_maybe_ada("libc.so.6"));
        assert!(!is_maybe_ada("main"));
        assert!(!is_maybe_ada("_ZN3foo3barEv"));
        assert!(!is_maybe_ada(""));
    }
}
