//! Demangler for the D programming language.
//!
//! The base structure (mangled-name walk, back references, `__S` fake
//! parents, and the magic compiler-generated symbol names) is a port of the
//! LLVM demangler for D, `DLangDemangle.cpp`
//! (<https://github.com/llvm/llvm-project/blob/main/llvm/lib/Demangle/DLangDemangle.cpp>),
//! which the LLVM Project distributes under the Apache License v2.0 with the
//! LLVM exception (`vendor/swift/LICENSE_LLVM.txt` carries the same license
//! text). The type grammar beyond LLVM's basic types (function types, member
//! functions, compound types, type modifiers, and template instances) follows
//! the D ABI specification, <https://dlang.org/spec/abi.html#name_mangling>.
//!
//! Rendering follows the conventions of the reference demanglers: function
//! symbols render as `module.func(params)` without a return type (which is
//! parsed but discarded), variables render as `type module.var`, and template
//! instances render as `name!(args)`.
//!
//! # Examples
//!
//! ```
//! # #[cfg(feature = "dlang")] {
//! use multi_demangle::{Demangle, DemangleOptions};
//! use symbolic_common::Name;
//!
//! let name = Name::from("_D6module4funcFZv");
//! assert_eq!(name.detect_language(), symbolic_common::Language::D);
//! assert_eq!(
//!     name.try_demangle(DemangleOptions::complete()),
//!     "module.func()"
//! );
//! # }
//! ```

// Entry points are feature-gated; without the feature the module only
// provides detection predicates, and the rest is legitimately dead.
#![cfg_attr(not(feature = "dlang"), allow(dead_code))]

use crate::DemangleOptions;

/// Recursion guard: the D grammar is recursive through types, template
/// instances, and back references, so a crafted input must not be able to
/// drive unbounded recursion.
const MAX_DEPTH: u32 = 128;

/// Rendering bound: a valid symbol never needs anywhere near this much
/// output, while substitution-heavy inputs must not balloon memory.
const MAX_OUTPUT: usize = 8192;

/// What the parse learned about the symbol's entity kind. Mirrored onto
/// [`crate::DemangledKind`] by the structured extractor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DlangKind {
    /// A plain function (parameters were part of the qualified name).
    Function,
    /// A member function (the `M` this-pointer marker).
    Method,
    /// A variable or other data symbol (a trailing type, no parameter list).
    Variable,
    /// A static initializer (`__init`).
    Initializer,
    /// A vtable symbol (`__vtbl`).
    VirtualTable,
    /// A class/interface info symbol (`__Class`/`__Interface`).
    TypeInfo,
    /// A module info symbol (`__ModuleInfo`).
    ModuleInfo,
}

/// Structure extracted alongside the rendering, consumed by the structured
/// extractor (`src/structured/dlang.rs`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DlangParts {
    pub namespace: Vec<String>,
    pub name: String,
    pub kind: Option<DlangKind>,
    pub parameters: Option<Vec<String>>,
    pub template_args: Option<Vec<String>>,
}

/// Cheap prefix check: a mangled D name is `_Dmain` or `_D` followed by a
/// qualified name, which always starts with a digit (length-prefixed
/// identifier or anonymous `0`) or a `Q` back reference.
pub(crate) fn is_maybe_dlang(symbol: &str) -> bool {
    if symbol == "_Dmain" {
        return true;
    }
    match symbol.as_bytes() {
        [b'_', b'D', rest @ ..] => {
            matches!(rest.first(), Some(b) if b.is_ascii_digit() || *b == b'Q')
        }
        _ => false,
    }
}

/// Decodes a back reference position (base 26; uppercase letters are higher
/// digits, a lowercase letter terminates) at `pos`. Returns the positive
/// value and the position after the letters.
fn decode_backref_pos_at(input: &[u8], mut pos: usize) -> Option<(usize, usize)> {
    let mut val: usize = 0;
    while let Some(&b) = input.get(pos) {
        if !b.is_ascii_alphabetic() {
            return None;
        }
        val = val.checked_mul(26)?;
        if b.is_ascii_lowercase() {
            val += (b - b'a') as usize;
            pos += 1;
            return if val == 0 { None } else { Some((val, pos)) };
        }
        val += (b - b'A') as usize;
        pos += 1;
    }
    None
}

/// Decodes the `Q` back reference whose `Q` sits at `qpos`, returning the
/// absolute input position it points to and the position after the encoding.
fn decode_backref_at(input: &[u8], qpos: usize) -> Option<(usize, usize)> {
    let (refpos, after) = decode_backref_pos_at(input, qpos + 1)?;
    if refpos > qpos {
        return None;
    }
    Some((qpos - refpos, after))
}

/// Whether a qualified name continues at `pos` (LLVM's `isSymbolName`, plus
/// the lengthless template form): a digit, a `Q` back reference that points
/// at a digit, or a `__T`/`__U` template instance.
fn is_symbol_name_at(input: &[u8], pos: usize) -> bool {
    match input.get(pos) {
        Some(b) if b.is_ascii_digit() => true,
        Some(b'_') => {
            matches!(input.get(pos..), Some(rest) if rest.starts_with(b"__T") || rest.starts_with(b"__U"))
        }
        Some(b'Q') => decode_backref_at(input, pos)
            .is_some_and(|(pointed, _)| input.get(pointed).is_some_and(|b| b.is_ascii_digit())),
        _ => false,
    }
}

/// The rendering of a basic type byte; `Some("")` marks `typeof(null)`.
fn basic_type_name(b: u8) -> Option<&'static str> {
    Some(match b {
        b'v' => "void",
        b'b' => "bool",
        b'g' => "byte",
        b'h' => "ubyte",
        b's' => "short",
        b't' => "ushort",
        b'i' => "int",
        b'k' => "uint",
        b'l' => "long",
        b'm' => "ulong",
        b'f' => "float",
        b'd' => "double",
        b'e' => "real",
        b'o' => "ifloat",
        b'p' => "idouble",
        b'j' => "ireal",
        b'q' => "cfloat",
        b'r' => "cdouble",
        b'c' => "creal",
        b'a' => "char",
        b'u' => "wchar",
        b'w' => "dchar",
        b'n' => "",
        _ => return None,
    })
}

struct Demangler<'a> {
    input: &'a [u8],
    pos: usize,
    /// Position that type back references may not reach (recursion guard),
    /// mirroring LLVM's `LastBackref`.
    last_backref: usize,
    failed: bool,
    depth: u32,
    /// Parameter renderings of the symbol's own function signature, captured
    /// when the signature is parsed at the top level.
    parameters: Option<Vec<String>>,
    /// Template arguments of the symbol's leaf component.
    template_args: Option<Vec<String>>,
    kind: Option<DlangKind>,
}

impl<'a> Demangler<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
            last_backref: input.len(),
            failed: false,
            depth: 0,
            parameters: None,
            template_args: None,
            kind: None,
        }
    }

    fn fail(&mut self) {
        self.failed = true;
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.input.get(self.pos + ahead).copied()
    }

    fn starts_with(&self, prefix: &[u8]) -> bool {
        self.input[self.pos..].starts_with(prefix)
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Enters one grammar recursion level; fails when the bound is exceeded.
    fn enter(&mut self) -> bool {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.fail();
            false
        } else {
            true
        }
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// Checks the output bound at choke points; a saturated rendering fails
    /// the whole demangling rather than truncating.
    fn check_bound(&mut self, out: &str) {
        if out.len() > MAX_OUTPUT {
            self.fail();
        }
    }

    // -- numbers and back references (LLVM port) ---------------------------

    /// Parses a decimal number. Like LLVM's `decodeNumber`, the number must
    /// be followed by at least one more byte; values above `u32::MAX` fail.
    fn decode_number(&mut self) -> Option<u32> {
        let mut val: u32 = 0;
        let mut digits = 0;
        while let Some(b) = self.peek() {
            if !b.is_ascii_digit() {
                break;
            }
            val = val.checked_mul(10)?.checked_add((b - b'0') as u32)?;
            digits += 1;
            self.pos += 1;
        }
        if digits == 0 || self.peek().is_none() {
            self.fail();
            return None;
        }
        Some(val)
    }

    /// Decodes a `Q` back reference at the current position, returning the
    /// absolute input position it points to.
    fn decode_backref(&mut self) -> Option<usize> {
        let (pointed, after) = decode_backref_at(self.input, self.pos)?;
        self.pos = after;
        Some(pointed)
    }

    // -- symbol parsing ----------------------------------------------------

    /// Parses a qualified name into `out`, dot-separated. With `is_root`,
    /// function signatures of components are consumed (a type context's
    /// qualified name never carries one — an `F` after it belongs to the
    /// enclosing grammar) and signatures and template arguments feed the
    /// structured view.
    fn parse_qualified(&mut self, out: &mut String, is_root: bool) {
        if !self.enter() {
            return;
        }
        let mut not_first = false;
        loop {
            if self.peek() == Some(b'0') {
                // Anonymous symbols: skip the run of zeros, emit nothing.
                while self.peek() == Some(b'0') {
                    self.pos += 1;
                }
            } else {
                if not_first {
                    out.push('.');
                }
                not_first = true;
                self.parse_identifier(out, is_root);
                if self.failed {
                    break;
                }
                self.check_bound(out);

                // A member-function marker (or a bare calling convention)
                // attaches this component's parameter list.
                if is_root
                    && matches!(
                        self.peek(),
                        Some(b'M') | Some(b'F') | Some(b'U') | Some(b'W') | Some(b'R') | Some(b'Y')
                    )
                {
                    let member = self.peek() == Some(b'M');
                    if member {
                        self.pos += 1;
                        // Optional type modifiers of the this-parameter.
                        self.skip_type_modifiers();
                    }
                    let Some((rendered, params)) = self.parse_function_params() else {
                        break;
                    };
                    out.push('(');
                    out.push_str(&rendered);
                    out.push(')');
                    if is_root {
                        self.parameters = Some(params);
                        self.kind = Some(if member {
                            DlangKind::Method
                        } else {
                            DlangKind::Function
                        });
                    }
                    self.check_bound(out);
                }
            }
            if self.failed || !is_symbol_name_at(self.input, self.pos) {
                break;
            }
        }
        self.leave();
    }

    /// Parses one identifier (length-prefixed name, back reference, template
    /// instance, or anonymous symbol) and appends it to `out`.
    fn parse_identifier(&mut self, out: &mut String, is_root: bool) {
        if !self.enter() {
            return;
        }
        if self.peek().is_none() {
            self.fail();
            self.leave();
            return;
        }

        if self.peek() == Some(b'Q') {
            self.parse_symbol_backref(out, is_root);
            self.leave();
            return;
        }

        // Template instances without a length prefix (`__T`/`__U`).
        if self.starts_with(b"__T") || self.starts_with(b"__U") {
            self.parse_template(out, is_root, None);
            self.leave();
            return;
        }

        let Some(len) = self.decode_number() else {
            self.leave();
            return;
        };
        let len = len as usize;
        if len == 0 || self.input.len() - self.pos < len {
            self.fail();
            self.leave();
            return;
        }

        // Template instances with a length prefix; the length covers the
        // whole `__T...Z` block.
        if len >= 5 && (self.starts_with(b"__T") || self.starts_with(b"__U")) {
            self.parse_template(out, is_root, Some(len));
            self.leave();
            return;
        }

        // Multiple different declarations in the same function disambiguate
        // through a fake parent `__Sddd`, which is skipped entirely (LLVM).
        if len >= 4 && self.starts_with(b"__S") {
            let mut digits_end = self.pos + 3;
            while digits_end < self.pos + len && self.input[digits_end].is_ascii_digit() {
                digits_end += 1;
            }
            if digits_end == self.pos + len {
                self.pos += len;
                self.parse_identifier(out, is_root);
                self.leave();
                return;
            }
            // Otherwise a plain identifier; fall through.
        }

        self.parse_lname(out, is_root, len);
        self.leave();
    }

    /// Renders the identifier body of length `len`, with special handling of
    /// the compiler-generated magic names.
    fn parse_lname(&mut self, out: &mut String, is_root: bool, len: usize) {
        // Prefix-form magic names render as "<prefix><path so far>", with
        // the dot of the intercepted component (or, on a degenerate
        // first-component match, one character of the prefix) dropped —
        // mirroring the reference implementation.
        let prefix = match len {
            6 if self.starts_with(b"__initZ") => Some("initializer for "),
            6 if self.starts_with(b"__vtblZ") => Some("vtable for "),
            7 if self.starts_with(b"__ClassZ") => Some("ClassInfo for "),
            11 if self.starts_with(b"__InterfaceZ") => Some("Interface for "),
            12 if self.starts_with(b"__ModuleInfoZ") => Some("ModuleInfo for "),
            _ => None,
        };
        if let Some(prefix) = prefix {
            let mut prefixed = String::with_capacity(out.len() + prefix.len());
            prefixed.push_str(prefix);
            prefixed.push_str(out);
            prefixed.pop();
            *out = prefixed;

            if is_root {
                self.kind = Some(match prefix {
                    "initializer for " => DlangKind::Initializer,
                    "vtable for " => DlangKind::VirtualTable,
                    "ModuleInfo for " => DlangKind::ModuleInfo,
                    _ => DlangKind::TypeInfo,
                });
            }
            self.pos += len;
            return;
        }

        // Leaf-replacement magic names.
        let leaf = match len {
            6 if self.starts_with(b"__ctor") => Some("this"),
            6 if self.starts_with(b"__dtor") => Some("~this"),
            10 if self.starts_with(b"__postblitMFZ") => Some("this(this)"),
            _ => None,
        };
        if let Some(leaf) = leaf {
            out.push_str(leaf);
            if is_root {
                self.kind = Some(DlangKind::Method);
                // `__postblitMFZ` carries its (empty) signature in the name.
                self.parameters = Some(Vec::new());
            }
            self.pos += len + if leaf == "this(this)" { 3 } else { 0 };
            return;
        }

        match std::str::from_utf8(&self.input[self.pos..self.pos + len]) {
            Ok(name) => out.push_str(name),
            Err(_) => {
                self.fail();
                return;
            }
        }
        self.pos += len;
    }

    /// Parses a `Q` back-referenced identifier and appends it to `out`.
    fn parse_symbol_backref(&mut self, out: &mut String, is_root: bool) {
        let Some(pointed) = self.decode_backref() else {
            self.fail();
            return;
        };
        let real_pos = self.pos;
        // The back reference points at a plain identifier (`<len><name>`).
        self.pos = pointed;
        let Some(len) = self.decode_number() else {
            self.pos = real_pos;
            return;
        };
        let len = len as usize;
        if len == 0 || self.input.len() - self.pos < len {
            self.fail();
            self.pos = real_pos;
            return;
        }
        let after_name = self.pos + len;
        self.parse_lname(out, is_root, len);
        // A back reference that reaches the end of the input fails like the
        // reference implementation.
        if after_name >= self.input.len() {
            self.fail();
        }
        self.pos = real_pos;
    }

    /// Parses a template instance `(__T|__U) LName TemplateArgs Z` and
    /// appends `name!(args)` to `out`. With `len`, the whole block must span
    /// exactly `len` bytes.
    fn parse_template(&mut self, out: &mut String, is_root: bool, len: Option<usize>) {
        if !self.enter() {
            return;
        }
        let start = self.pos;
        self.pos += 3; // `__T` / `__U`

        // The template's own identifier.
        if !is_symbol_name_at(self.input, self.pos) || self.peek() == Some(b'0') {
            self.fail();
            self.leave();
            return;
        }
        let mut name = String::new();
        self.parse_identifier(&mut name, false);
        if self.failed {
            self.leave();
            return;
        }

        let args = match self.parse_template_args() {
            Some(args) => args,
            None => {
                self.leave();
                return;
            }
        };

        out.push_str(&name);
        out.push_str("!(");
        out.push_str(&args.join(", "));
        out.push(')');

        if let Some(len) = len {
            if self.pos - start != len {
                self.fail();
            }
        }
        if is_root {
            self.template_args = Some(args);
        }
        self.check_bound(out);
        self.leave();
    }

    /// Parses template arguments up to the closing `Z`, returning their
    /// renderings.
    fn parse_template_args(&mut self) -> Option<Vec<String>> {
        if !self.enter() {
            return None;
        }
        let mut args = Vec::new();
        loop {
            if self.eat(b'H') {
                // A specialized-parameter prefix; the argument follows.
            }
            let mut arg = String::new();
            match self.peek() {
                Some(b'Z') => {
                    self.pos += 1;
                    self.leave();
                    return Some(args);
                }
                Some(b'T') => {
                    self.pos += 1;
                    if !self.parse_type(&mut arg) {
                        self.fail();
                    }
                }
                Some(b'V') => {
                    self.pos += 1;
                    // The value's type is parsed; only its letter (through
                    // any back reference) and, for struct literals, its
                    // rendering reach the output.
                    let mut type_char = self.peek();
                    if type_char == Some(b'Q') {
                        type_char = decode_backref_at(self.input, self.pos)
                            .and_then(|(pointed, _)| self.input.get(pointed).copied());
                    }
                    let mut type_name = String::new();
                    if !self.parse_type(&mut type_name) {
                        self.fail();
                    }
                    if !self.failed && !self.parse_value(&mut arg, type_char, &type_name) {
                        self.fail();
                    }
                }
                Some(b'S') => {
                    self.pos += 1;
                    self.parse_qualified(&mut arg, false);
                }
                Some(b'X') => {
                    self.pos += 1;
                    let len = match self.decode_number() {
                        Some(len) => len as usize,
                        None => {
                            self.fail();
                            self.leave();
                            return None;
                        }
                    };
                    if !self.failed {
                        if self.input.len() - self.pos < len {
                            self.fail();
                        } else {
                            arg.push_str(&String::from_utf8_lossy(
                                &self.input[self.pos..self.pos + len],
                            ));
                            self.pos += len;
                        }
                    }
                }
                _ => self.fail(),
            }
            if self.failed {
                self.leave();
                return None;
            }
            self.check_bound(&arg);
            args.push(arg);
        }
    }

    /// Parses a template value. `type_char` (when a basic type) decides
    /// whether an `A` literal is a plain or an associative array; `type_name`
    /// prefixes struct literals with their type.
    fn parse_value(&mut self, out: &mut String, type_char: Option<u8>, type_name: &str) -> bool {
        if !self.enter() {
            return false;
        }
        let ok = self.parse_value_inner(out, type_char, type_name);
        self.leave();
        if ok {
            self.check_bound(out);
        }
        ok
    }

    fn parse_value_inner(
        &mut self,
        out: &mut String,
        type_char: Option<u8>,
        type_name: &str,
    ) -> bool {
        match self.peek() {
            Some(b'n') => {
                self.pos += 1;
                out.push_str("null");
            }
            Some(b'N') => {
                self.pos += 1;
                out.push('-');
                return self.parse_integer(out);
            }
            Some(b'i') => {
                self.pos += 1;
                return self.parse_integer(out);
            }
            Some(b'e') => {
                self.pos += 1;
                return self.parse_hex_float(out);
            }
            Some(b'c') => {
                self.pos += 1;
                if !self.parse_hex_float(out) || !self.eat(b'c') {
                    return false;
                }
                out.push('+');
                if !self.parse_hex_float(out) {
                    return false;
                }
                out.push('i');
            }
            Some(b'a') | Some(b'w') | Some(b'd') => return self.parse_string_value(out),
            Some(b'A') => {
                self.pos += 1;
                return self.parse_array_literal(out, type_char == Some(b'H'));
            }
            Some(b'S') => {
                self.pos += 1;
                let Some(count) = self.decode_number() else {
                    return false;
                };
                out.push_str(type_name);
                out.push('(');
                for i in 0..count {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    if !self.parse_value(out, None, "") {
                        return false;
                    }
                }
                out.push(')');
            }
            Some(b'f') => {
                self.pos += 1;
                // A function literal symbol: a full mangled name.
                if !self.starts_with(b"_D") || !is_symbol_name_at(self.input, self.pos + 2) {
                    return false;
                }
                self.pos += 2;
                let mut path = String::new();
                self.parse_qualified(&mut path, true);
                if self.failed {
                    return false;
                }
                // Artificial symbols end with 'Z'; otherwise the trailing
                // type is parsed and discarded.
                if self.peek() == Some(b'Z') {
                    self.pos += 1;
                } else {
                    let mut ty = String::new();
                    if !self.parse_type(&mut ty) {
                        return false;
                    }
                }
                out.push_str(&path);
            }
            _ => return false,
        }
        true
    }

    /// Parses a decimal integer literal (digits may run to the end of the
    /// value's extent, so the strict trailing-byte rule does not apply).
    fn parse_integer(&mut self, out: &mut String) -> bool {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if !b.is_ascii_digit() {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            return false;
        }
        out.push_str(&String::from_utf8_lossy(&self.input[start..self.pos]));
        true
    }

    /// Parses a hex float (`NAN`, `INF`, `NINF`, or `[N]HexDigits P [N]Exp`)
    /// and renders it verbatim.
    fn parse_hex_float(&mut self, out: &mut String) -> bool {
        let start = self.pos;
        if self.starts_with(b"NAN") || self.starts_with(b"INF") {
            self.pos += 3;
        } else if self.starts_with(b"NINF") {
            self.pos += 4;
        } else {
            if self.peek() == Some(b'N') {
                self.pos += 1;
            }
            let mut digits = 0;
            while let Some(b) = self.peek() {
                if b.is_ascii_hexdigit() {
                    self.pos += 1;
                    digits += 1;
                } else {
                    break;
                }
            }
            if digits == 0 || !self.eat(b'P') {
                return false;
            }
            if self.peek() == Some(b'N') {
                self.pos += 1;
            }
            while let Some(b) = self.peek() {
                if b.is_ascii_digit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }
        out.push_str(&String::from_utf8_lossy(&self.input[start..self.pos]));
        true
    }

    /// Parses a string literal value: `<width><count>_<hex>`, decoded per
    /// character width (`a` UTF-8, `w` UTF-16, `d` UTF-32) and rendered
    /// quoted.
    fn parse_string_value(&mut self, out: &mut String) -> bool {
        let Some(width) = self.peek() else {
            return false;
        };
        self.pos += 1;
        let Some(count) = self.decode_number() else {
            return false;
        };
        if !self.eat(b'_') {
            return false;
        }
        let count = count as usize;
        if count > MAX_OUTPUT {
            self.fail();
            return false;
        }

        let read_hex = |dem: &mut Self, bytes_needed: usize| -> Option<Vec<u8>> {
            let mut bytes = Vec::with_capacity(bytes_needed);
            for _ in 0..bytes_needed {
                let hi = (*dem.input.get(dem.pos)? as char).to_digit(16)?;
                let lo = (*dem.input.get(dem.pos + 1)? as char).to_digit(16)?;
                bytes.push((hi * 16 + lo) as u8);
                dem.pos += 2;
            }
            Some(bytes)
        };

        let decoded: String = match width {
            b'a' => {
                let bytes = match read_hex(self, count) {
                    Some(bytes) => bytes,
                    None => return false,
                };
                match String::from_utf8(bytes) {
                    Ok(s) => s,
                    Err(_) => {
                        self.fail();
                        return false;
                    }
                }
            }
            b'w' => {
                let bytes = match read_hex(self, count * 2) {
                    Some(bytes) => bytes,
                    None => return false,
                };
                let units: Vec<u16> = bytes
                    .chunks(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                match String::from_utf16(&units) {
                    Ok(s) => s,
                    Err(_) => {
                        self.fail();
                        return false;
                    }
                }
            }
            b'd' => {
                let bytes = match read_hex(self, count * 4) {
                    Some(bytes) => bytes,
                    None => return false,
                };
                let mut s = String::new();
                for chunk in bytes.chunks(4) {
                    if chunk.len() < 4 {
                        self.fail();
                        return false;
                    }
                    let v = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    match char::from_u32(v) {
                        Some(c) => s.push(c),
                        None => {
                            self.fail();
                            return false;
                        }
                    }
                }
                s
            }
            _ => return false,
        };

        out.push('"');
        for c in decoded.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        true
    }

    /// Parses an array or associative array literal: `A<count> <values>`
    /// (assoc: `key: value` pairs).
    fn parse_array_literal(&mut self, out: &mut String, assoc: bool) -> bool {
        let Some(count) = self.decode_number() else {
            return false;
        };
        if count as usize > MAX_OUTPUT {
            self.fail();
            return false;
        }
        out.push('[');
        for i in 0..count {
            if i > 0 {
                out.push_str(", ");
            }
            if assoc {
                if !self.parse_value(out, None, "") {
                    return false;
                }
                out.push(':');
            }
            if !self.parse_value(out, None, "") {
                return false;
            }
            self.check_bound(out);
        }
        out.push(']');
        true
    }

    // -- function signatures ------------------------------------------------

    /// Parses the calling convention, attributes, parameter list, and
    /// variadic marker of a function signature, returning the rendered
    /// parameter list and the individual parameter renderings.
    fn parse_function_params(&mut self) -> Option<(String, Vec<String>)> {
        // Calling convention: F D, U C, W Windows, R C++, Y Objective-C.
        match self.peek() {
            Some(b'F') | Some(b'U') | Some(b'W') | Some(b'R') | Some(b'Y') => self.pos += 1,
            _ => {
                self.fail();
                return None;
            }
        }
        self.parse_func_attrs();

        let mut params: Vec<String> = Vec::new();
        let mut variadic = false;
        loop {
            match self.peek() {
                Some(b'Z') => {
                    self.pos += 1;
                    break;
                }
                Some(b'X') | Some(b'Y') => {
                    self.pos += 1;
                    variadic = true;
                    break;
                }
                Some(b'M') => {
                    // scope parameter
                    self.pos += 1;
                    let mut rendered = String::from("scope ");
                    if !self.parse_parameter2(&mut rendered) {
                        self.fail();
                        return None;
                    }
                    params.push(rendered);
                }
                Some(b'N') if self.peek_at(1) == Some(b'k') => {
                    // return parameter
                    self.pos += 2;
                    let mut rendered = String::from("return ");
                    if !self.parse_parameter2(&mut rendered) {
                        self.fail();
                        return None;
                    }
                    params.push(rendered);
                }
                _ => {
                    let mut rendered = String::new();
                    if !self.parse_parameter2(&mut rendered) {
                        self.fail();
                        return None;
                    }
                    params.push(rendered);
                }
            }
            if params.len() > 1024 {
                self.fail();
                return None;
            }
        }

        let mut rendered = params.join(", ");
        if variadic {
            if rendered.is_empty() {
                rendered.push_str("...");
            } else {
                rendered.push_str(", ...");
            }
        }
        Some((rendered, params))
    }

    /// Consumes function attribute letters (`Na` pure, `Nb` nothrow, `Nc`
    /// ref, `Nd` property, `Ne` trusted, `Nf` safe, `Ni` nogc, `Nj` return,
    /// `Nl` scope, `Nm` live).
    fn parse_func_attrs(&mut self) {
        while self.peek() == Some(b'N')
            && matches!(
                self.peek_at(1),
                Some(b'a')
                    | Some(b'b')
                    | Some(b'c')
                    | Some(b'd')
                    | Some(b'e')
                    | Some(b'f')
                    | Some(b'i')
                    | Some(b'j')
                    | Some(b'l')
                    | Some(b'm')
            )
        {
            self.pos += 2;
        }
    }

    /// Parses one parameter without the scope/return markers.
    fn parse_parameter2(&mut self, out: &mut String) -> bool {
        match self.peek() {
            Some(b'I') => {
                self.pos += 1;
                out.push_str("in ");
            }
            Some(b'J') => {
                self.pos += 1;
                out.push_str("out ");
            }
            Some(b'K') => {
                self.pos += 1;
                out.push_str("ref ");
            }
            Some(b'L') => {
                self.pos += 1;
                out.push_str("lazy ");
            }
            _ => {}
        }
        self.parse_type(out)
    }

    // -- types ---------------------------------------------------------------

    /// Skips (without rendering) a type modifier sequence; used after the
    /// member-function `M` marker.
    fn skip_type_modifiers(&mut self) {
        loop {
            match (self.peek(), self.peek_at(1)) {
                (Some(b'O'), _) | (Some(b'x'), _) | (Some(b'y'), _) => self.pos += 1,
                (Some(b'N'), Some(b'g')) => self.pos += 2,
                _ => break,
            }
        }
    }

    /// Parses type modifiers into `mods` (rendered as words). Returns whether
    /// any modifier was seen.
    fn parse_type_modifiers(&mut self, mods: &mut Vec<&'static str>) -> bool {
        let before = mods.len();
        loop {
            match (self.peek(), self.peek_at(1)) {
                (Some(b'O'), _) => {
                    self.pos += 1;
                    mods.push("shared");
                }
                (Some(b'x'), _) => {
                    self.pos += 1;
                    mods.push("const");
                }
                (Some(b'N'), Some(b'g')) => {
                    self.pos += 2;
                    mods.push("inout");
                }
                (Some(b'y'), _) => {
                    self.pos += 1;
                    mods.push("immutable");
                }
                _ => break,
            }
        }
        mods.len() > before
    }

    /// Parses a type and appends its rendering to `out`.
    fn parse_type(&mut self, out: &mut String) -> bool {
        if !self.enter() {
            return false;
        }
        let ok = self.parse_type_inner(out);
        self.leave();
        if ok && !self.failed {
            self.check_bound(out);
        }
        ok
    }

    fn parse_type_inner(&mut self, out: &mut String) -> bool {
        // Type back reference.
        if self.peek() == Some(b'Q') {
            self.parse_type_backref(out);
            return !self.failed;
        }

        // Type modifiers prefix the rendering of the modified type. Only the
        // text this call appends may be wrapped: `out` can already carry a
        // caller's prefix (a parameter storage class such as `ref `), and
        // wrapping that too would render `Kx S` as `const(ref S)` instead of
        // `ref const(S)`.
        let mut mods = Vec::new();
        if self.parse_type_modifiers(&mut mods) {
            let start = out.len();
            if !self.parse_type(out) {
                return false;
            }
            let inner = out.split_off(start);
            out.push_str(&format!("{}({})", mods.join(" "), inner));
            return true;
        }

        match self.peek() {
            // Compound types.
            Some(b'A') => {
                self.pos += 1;
                if !self.parse_type(out) {
                    return false;
                }
                out.push_str("[]");
            }
            Some(b'G') => {
                // Static array: `G <dimension> <element type>`, rendered
                // `<element type>[<dimension>]`. The dimension comes first in
                // the mangling — `G4i` is `int[4]` (verified against
                // `c++filt -s dlang` on gdc/ldc output).
                self.pos += 1;
                let Some(dim) = self.decode_number() else {
                    return false;
                };
                if !self.parse_type(out) {
                    return false;
                }
                out.push('[');
                out.push_str(&dim.to_string());
                out.push(']');
            }
            Some(b'H') => {
                // Associative array `H <key> <value>` -> `value[key]`. As
                // above, only the value text appended here is rewritten.
                self.pos += 1;
                let mut key = String::new();
                let start = out.len();
                if !self.parse_type(&mut key) || !self.parse_type(out) {
                    return false;
                }
                let value = out.split_off(start);
                out.push_str(&format!("{value}[{key}]"));
            }
            Some(b'P') => {
                self.pos += 1;
                // `P` applied to a function type is how D spells a function
                // pointer (`PFiZi` is `int function(int)`). The function
                // rendering already reads as a pointer, so appending `*`
                // would produce `int function(int)*`.
                let points_to_function = self.peek() == Some(b'F');
                if !self.parse_type(out) {
                    return false;
                }
                if !points_to_function {
                    out.push('*');
                }
            }
            Some(b'R') => {
                self.pos += 1;
                if !self.parse_type(out) {
                    return false;
                }
                out.push('&');
            }
            Some(b'D') => {
                // Delegate: D TypeModifiers? TypeFunction.
                self.pos += 1;
                let mut mods = Vec::new();
                self.parse_type_modifiers(&mut mods);
                let Some((params, _)) = self.parse_function_params() else {
                    return false;
                };
                let mut ret = String::new();
                if !self.parse_type(&mut ret) {
                    return false;
                }
                let mut sig = format!("{ret} delegate({params})");
                if !mods.is_empty() {
                    sig = format!("{}({})", mods.join(" "), sig);
                }
                out.push_str(&sig);
            }
            Some(b'B') => {
                // Tuple: B Parameters Z.
                self.pos += 1;
                out.push('(');
                loop {
                    match self.peek() {
                        Some(b'Z') => {
                            self.pos += 1;
                            break;
                        }
                        Some(b'M') => {
                            self.pos += 1;
                            let mut rendered = String::from("scope ");
                            if !self.parse_parameter2(&mut rendered) {
                                return false;
                            }
                            out.push_str(&rendered);
                        }
                        Some(b'N') if self.peek_at(1) == Some(b'k') => {
                            self.pos += 2;
                            let mut rendered = String::from("return ");
                            if !self.parse_parameter2(&mut rendered) {
                                return false;
                            }
                            out.push_str(&rendered);
                        }
                        _ => {
                            let mut rendered = String::new();
                            if !self.parse_parameter2(&mut rendered) {
                                return false;
                            }
                            out.push_str(&rendered);
                        }
                    }
                    match self.peek() {
                        Some(b'Z') => {}
                        Some(b'X') | Some(b'Y') => {
                            self.pos += 1;
                            out.push_str(", ...");
                            if !self.eat(b'Z') {
                                return false;
                            }
                            break;
                        }
                        _ => out.push_str(", "),
                    }
                }
                out.push(')');
            }
            Some(b'N') => match self.peek_at(1) {
                Some(b'h') => {
                    // Vector type.
                    self.pos += 2;
                    if !self.parse_type(out) {
                        return false;
                    }
                    let rendered = format!("__vector({out})");
                    out.clear();
                    out.push_str(&rendered);
                }
                Some(b'n') => {
                    self.pos += 2;
                    out.push_str("noreturn");
                }
                _ => return false,
            },
            // User-defined types carry a qualified name.
            Some(b'C') | Some(b'S') | Some(b'E') | Some(b'T') | Some(b'I') => {
                self.pos += 1;
                self.parse_qualified(out, false);
                if self.failed {
                    return false;
                }
            }
            // Function types appearing as values (function pointers, ...).
            Some(b'F') | Some(b'U') | Some(b'W') | Some(b'Y') => {
                if !self.parse_function_type_into(out) {
                    return false;
                }
            }
            // Basic types.
            Some(b'z') => match self.peek_at(1) {
                Some(b'i') => {
                    self.pos += 2;
                    out.push_str("cent");
                }
                Some(b'k') => {
                    self.pos += 2;
                    out.push_str("ucent");
                }
                _ => return false,
            },
            Some(b) => match basic_type_name(b) {
                // typeof(null) is recognized but renders nothing.
                Some("") => self.pos += 1,
                Some(name) => {
                    self.pos += 1;
                    out.push_str(name);
                }
                None => return false,
            },
            None => return false,
        }
        true
    }

    /// Parses a full function type (calling convention, attributes,
    /// parameters, close, return type) rendering `Ret function(params)`.
    fn parse_function_type_into(&mut self, out: &mut String) -> bool {
        let Some((params, _)) = self.parse_function_params() else {
            return false;
        };
        let mut ret = String::new();
        if !self.parse_type(&mut ret) {
            return false;
        }
        out.push_str(&ret);
        out.push_str(" function(");
        out.push_str(&params);
        out.push(')');
        true
    }

    /// Parses a `Q` back-referenced type and appends it to `out`.
    fn parse_type_backref(&mut self, out: &mut String) {
        if self.pos >= self.last_backref {
            self.fail();
            return;
        }
        let save_last = self.last_backref;
        self.last_backref = self.pos;
        let Some(pointed) = self.decode_backref() else {
            self.fail();
            return;
        };
        let real_pos = self.pos;
        self.pos = pointed;
        if !self.parse_type(out) {
            self.fail();
        }
        if self.pos >= self.input.len() {
            // The back-referenced type ran to the end of the input; the real
            // stream has nothing left either, so fail like the reference.
            self.fail();
        }
        self.pos = real_pos;
        self.last_backref = save_last;
    }
}

/// The outcome of a successful parse.
struct ParsedSymbol {
    /// The qualified path, with function parameters appended to the leaf
    /// (`module.func(int)`) and magic prefixes applied.
    path: String,
    parts: DlangParts,
    /// The variable type prefix rendering, when the symbol is data.
    type_prefix: Option<String>,
}

/// Runs the demangler over `symbol` and collects the rendering and structure.
fn parse_symbol(symbol: &str) -> Option<ParsedSymbol> {
    if symbol == "_Dmain" {
        return Some(ParsedSymbol {
            path: "D main".to_string(),
            parts: DlangParts {
                namespace: Vec::new(),
                name: "D main".to_string(),
                kind: Some(DlangKind::Function),
                parameters: None,
                template_args: None,
            },
            type_prefix: None,
        });
    }
    if !is_maybe_dlang(symbol) {
        return None;
    }

    let mut dem = Demangler::new(symbol);
    dem.pos = 2; // consume `_D`
    let mut path = String::new();
    dem.parse_qualified(&mut path, true);
    // A qualified name must be followed by either 'Z' or a type; running out
    // of input here fails like the reference implementation.
    if dem.failed || dem.peek().is_none() {
        return None;
    }

    // Artificial symbols end with 'Z' and have no type.
    let mut type_prefix = None;
    if dem.peek() == Some(b'Z') {
        dem.pos += 1;
    } else if dem.peek().is_some() {
        // The trailing type is a variable's type or a function's return
        // type; the latter is parsed and discarded, matching the reference
        // demanglers.
        let mut ty = String::new();
        if !dem.parse_type(&mut ty) || dem.failed {
            return None;
        }
        if dem.parameters.is_none() && !ty.is_empty() {
            type_prefix = Some(ty);
        }
    }
    // The entire symbol must have been consumed.
    if dem.pos != dem.input.len() {
        return None;
    }

    let mut parts = split_parts(&path);
    parts.kind = Some(dem.kind.unwrap_or_else(|| {
        if dem.parameters.is_some() {
            DlangKind::Function
        } else {
            DlangKind::Variable
        }
    }));
    parts.parameters = dem.parameters.clone();
    parts.template_args = dem.template_args.clone();

    Some(ParsedSymbol {
        path,
        parts,
        type_prefix,
    })
}

/// Splits a rendered D path into its namespace components and leaf name,
/// separating the leaf's parameter list. Template arguments stay part of the
/// leaf name (`temp!(int)`); the structured extractor splits them out using
/// the captured argument renderings.
fn split_parts(path: &str) -> DlangParts {
    // Dots at parenthesis depth zero are the only path separators; dots
    // inside `!(...)` groups or parameter types belong to their group.
    let mut dot_positions = Vec::new();
    let mut depth = 0usize;
    for (idx, ch) in path.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '.' if depth == 0 => dot_positions.push(idx),
            _ => {}
        }
    }

    let (namespace, leaf) = match dot_positions.last() {
        Some(&idx) => (
            path[..idx].split('.').map(str::to_string).collect(),
            &path[idx + 1..],
        ),
        None => (Vec::new(), path),
    };

    // A trailing parameter group on the leaf is a `(...)` not preceded by
    // `!` (which would mark a template argument group).
    let mut name = leaf.to_string();
    let mut parameters = None;
    if leaf.ends_with(')') {
        let mut depth = 0usize;
        for (idx, ch) in leaf.char_indices().rev() {
            if ch == ')' {
                depth += 1;
            } else if ch == '(' {
                depth -= 1;
                if depth == 0 {
                    if idx > 0 && leaf[..idx].ends_with('!') {
                        // Template argument group; keep it in the name.
                    } else {
                        parameters = Some(
                            leaf[idx + 1..leaf.len() - 1]
                                .split(", ")
                                .filter(|p| !p.is_empty())
                                .map(str::to_string)
                                .collect(),
                        );
                        name.truncate(idx);
                    }
                    break;
                }
            }
        }
    }

    DlangParts {
        namespace,
        name,
        kind: None,
        parameters,
        template_args: None,
    }
}

/// Demangles a D symbol with the given options.
pub(crate) fn demangle(symbol: &str, opts: DemangleOptions) -> Option<String> {
    let parsed = parse_symbol(symbol)?;

    // Name-only renderings drop the parameter list from the leaf.
    let base = if opts.parameters {
        parsed.path
    } else {
        let parts = split_parts(&parsed.path);
        let mut out = parts.namespace.join(".");
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(&parts.name);
        out
    };

    match parsed.type_prefix {
        // A variable's type renders as a prefix, subject to the return-type
        // option; functions have no return type in the rendering.
        Some(ty) if opts.return_type => Some(format!("{ty} {base}")),
        _ => Some(base),
    }
}

/// Returns the parse structure of a D symbol for the structured extractor.
pub(crate) fn structured_parts(symbol: &str) -> Option<DlangParts> {
    parse_symbol(symbol).map(|parsed| parsed.parts)
}

/// Splits a rendered template argument list on top-level commas; used as a
/// fallback when the parse did not capture the leaf's arguments.
pub(crate) fn split_template_args(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in text.char_indices() {
        match ch {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                args.push(text[start..idx].trim().to_string());
                start = idx + 1;
            }
            _ => {}
        }
    }
    if start < text.len() || !args.is_empty() {
        args.push(text[start..].trim().to_string());
    }
    args
}

#[cfg(test)]
mod test {
    use super::*;

    fn dem(sym: &str) -> String {
        demangle(sym, DemangleOptions::complete()).unwrap_or_else(|| panic!("demangles: {sym}"))
    }

    #[test]
    fn llvm_reference_suite() {
        // Cases from LLVM's DLangDemangleTest.cpp.
        for (symbol, expected) in [
            ("_Dmain", Some("D main")),
            ("_Z", None),
            ("_DDD", None),
            ("_D88", None),
            ("_D8demangleZ", Some("demangle")),
            ("_D8demangle4testZ", Some("demangle.test")),
            ("_D8demangle4test5test2Z", Some("demangle.test.test2")),
            ("_D8demangle4test0Z", Some("demangle.test")),
            ("_D8demangle4test03fooZ", Some("demangle.test.foo")),
            (
                "_D8demangle4test6__initZ",
                Some("initializer for demangle.test"),
            ),
            ("_D8demangle4test6__vtblZ", Some("vtable for demangle.test")),
            (
                "_D8demangle4test7__ClassZ",
                Some("ClassInfo for demangle.test"),
            ),
            (
                "_D8demangle4test11__InterfaceZ",
                Some("Interface for demangle.test"),
            ),
            (
                "_D8demangle4test12__ModuleInfoZ",
                Some("ModuleInfo for demangle.test"),
            ),
            ("_D8demangle4__S14testZ", Some("demangle.test")),
            ("_D8demangle4__Sd4testZ", Some("demangle.__Sd.test")),
            ("_D8demangle3fooi", Some("int demangle.foo")),
            ("_D8demangle3foov", Some("void demangle.foo")),
            ("_D8demangle3foo", None),
            ("_D8demangle3fooinvalidtypeseq", None),
            ("_D8demangle3ABCQe1ai", Some("int demangle.ABC.ABC.a")),
            ("_D8demangle3ABCQa1ai", None),
            ("_D8demangleQDXXXXXXXXXXXXx", None),
            ("_D8demangle4ABCi1aQd", Some("int demangle.ABCi.a")),
            ("_D8demangle3fooQXXXx", None),
            ("_D8demangle5recurQa", None),
            ("_D8demangle3fooa", Some("char demangle.foo")),
            ("_D8demangle3foob", Some("bool demangle.foo")),
            ("_D8demangle3fooc", Some("creal demangle.foo")),
            ("_D8demangle3food", Some("double demangle.foo")),
            ("_D8demangle3fooe", Some("real demangle.foo")),
            ("_D8demangle3foof", Some("float demangle.foo")),
            ("_D8demangle3foog", Some("byte demangle.foo")),
            ("_D8demangle3fooh", Some("ubyte demangle.foo")),
            ("_D8demangle3fooj", Some("ireal demangle.foo")),
            ("_D8demangle3fook", Some("uint demangle.foo")),
            ("_D8demangle3fool", Some("long demangle.foo")),
            ("_D8demangle3foom", Some("ulong demangle.foo")),
            ("_D8demangle3foon", Some("demangle.foo")),
            ("_D8demangle3fooo", Some("ifloat demangle.foo")),
            ("_D8demangle3foop", Some("idouble demangle.foo")),
            ("_D8demangle3fooq", Some("cfloat demangle.foo")),
            ("_D8demangle3foor", Some("cdouble demangle.foo")),
            ("_D8demangle3foos", Some("short demangle.foo")),
            ("_D8demangle3foot", Some("ushort demangle.foo")),
            ("_D8demangle3foou", Some("wchar demangle.foo")),
            ("_D8demangle3foow", Some("dchar demangle.foo")),
            ("_D8demangle3foozi", Some("cent demangle.foo")),
            ("_D8demangle3foozk", Some("ucent demangle.foo")),
            ("_D8demangle3fooNn", Some("noreturn demangle.foo")),
            ("_D8demangle3fooiabc", None),
            ("_D8demangle3foovxxx", None),
            ("_D8demangle3fooza", None),
            ("_D8demangle3fooNx", None),
        ] {
            match expected {
                Some(expected) => assert_eq!(dem(symbol), expected, "for {symbol}"),
                None => assert_eq!(
                    demangle(symbol, DemangleOptions::complete()),
                    None,
                    "for {symbol}"
                ),
            }
        }
    }

    #[test]
    fn function_symbols() {
        assert_eq!(dem("_D6module4funcFZv"), "module.func()");
        assert_eq!(dem("_D6module4funcFZi"), "module.func()");
        assert_eq!(dem("_D6module4funcFiZv"), "module.func(int)");
        assert_eq!(
            dem("_D6module4funcFikdZv"),
            "module.func(int, uint, double)"
        );
        assert_eq!(dem("_D6module4funcFPiaZv"), "module.func(int*, char)");
        assert_eq!(dem("_D6module4funcFAiZv"), "module.func(int[])");
        // Static arrays mangle the dimension before the element type;
        // `c++filt -s dlang` rejects the reverse spelling outright.
        assert_eq!(dem("_D6module4funcFG3iZv"), "module.func(int[3])");
        assert_eq!(dem("_D6module4funcFHiiZv"), "module.func(int[int])");
        assert_eq!(dem("_D6module4funcFxAiZv"), "module.func(const(int[]))");
        assert_eq!(dem("_D6module4funcFNbNafZv"), "module.func(float)");
        // Variadics.
        assert_eq!(dem("_D6module4funcFXv"), "module.func(...)");
        assert_eq!(dem("_D6module4funcFiXv"), "module.func(int, ...)");
        assert_eq!(dem("_D6module4funcFiYv"), "module.func(int, ...)");
        // Parameter storage classes.
        assert_eq!(dem("_D6module4funcFKiZv"), "module.func(ref int)");
        assert_eq!(dem("_D6module4funcFLiZv"), "module.func(lazy int)");
        assert_eq!(dem("_D6module4funcFMKiZv"), "module.func(scope ref int)");
        assert_eq!(dem("_D6module4funcFNkiZv"), "module.func(return int)");
        assert_eq!(dem("_D6module4funcFIiZv"), "module.func(in int)");
        assert_eq!(dem("_D6module4funcFJiZv"), "module.func(out int)");
    }

    #[test]
    fn member_functions() {
        assert_eq!(dem("_D6module4Test6methodMFZv"), "module.Test.method()");
        assert_eq!(dem("_D6module4Test6methodMFiZi"), "module.Test.method(int)");
        let parts = structured_parts("_D6module4Test6methodMFiZi").unwrap();
        assert_eq!(parts.kind, Some(DlangKind::Method));
        assert_eq!(parts.namespace, ["module", "Test"]);
        assert_eq!(parts.name, "method");
        assert_eq!(
            parts.parameters.as_deref(),
            Some(["int".to_string()].as_slice())
        );
    }

    #[test]
    fn symbol_backref() {
        // `Qi` points back at `3foo`; the back-referenced identifier fills
        // the leaf component.
        assert_eq!(dem("_D6module3foo3barQiFZv"), "module.foo.bar.foo()");
        let parts = structured_parts("_D6module3foo3barQiFZv").unwrap();
        assert_eq!(parts.namespace, ["module", "foo", "bar"]);
        assert_eq!(parts.name, "foo");
    }

    #[test]
    fn template_instances() {
        // A function inside a template instance.
        assert_eq!(dem("_D6module9__T4tempZ4funcFZv"), "module.temp!().func()");
        // Lengthless template form.
        assert_eq!(dem("_D6module__T4tempZ4funcFZv"), "module.temp!().func()");
        assert_eq!(
            dem("_D6module11__T4tempTiZ4funcFZv"),
            "module.temp!(int).func()"
        );
        assert_eq!(
            dem("_D6module13__T4tempTiTkZ4funcFZv"),
            "module.temp!(int, uint).func()"
        );
        // Value arguments.
        assert_eq!(
            dem("_D6module14__T4tempVii42Z4funcFZv"),
            "module.temp!(42).func()"
        );
        assert_eq!(
            dem("_D6module14__T4tempViN10Z4funcFZv"),
            "module.temp!(-10).func()"
        );
        assert_eq!(
            dem("_D6module12__T4tempVnnZ4funcFZv"),
            "module.temp!(null).func()"
        );
        assert_eq!(
            dem("_D6module24__T4tempVaa5_68656c6c6fZ4funcFZv"),
            "module.temp!(\"hello\").func()"
        );
        // Symbol arguments.
        assert_eq!(
            dem("_D6module22__T4tempS6module4TestZ4funcFZv"),
            "module.temp!(module.Test).func()"
        );
        // The length prefix must match the template block exactly.
        assert_eq!(
            demangle(
                "_D6module12__T4tempTiTkZ4funcFZv",
                DemangleOptions::complete()
            ),
            None
        );
    }

    #[test]
    fn magic_leaf_names() {
        assert_eq!(dem("_D6module4Test6__ctorFZv"), "module.Test.this()");
        assert_eq!(dem("_D6module4Test6__dtorFZv"), "module.Test.~this()");
        assert_eq!(
            dem("_D6module4Test10__postblitMFZv"),
            "module.Test.this(this)"
        );
    }

    #[test]
    fn variable_symbols() {
        assert_eq!(dem("_D6module7counteri"), "int module.counter");
        assert_eq!(
            demangle("_D6module7counteri", DemangleOptions::name_only()),
            Some("module.counter".to_string())
        );
        assert_eq!(
            demangle(
                "_D6module7counteri",
                DemangleOptions::complete().return_type(false)
            ),
            Some("module.counter".to_string())
        );
        assert_eq!(dem("_D6module4dataPAi"), "int[]* module.data");
    }

    /// Symbols taken verbatim from `nm` over ldc2/gdc output for
    /// `contrib/fixtures/dlang/corpus.d`, with the expected rendering
    /// cross-checked against `c++filt -s dlang` (libiberty's independent D
    /// demangler). Unlike hand-written cases these cannot encode a grammar
    /// the compiler does not actually emit.
    #[test]
    fn real_compiler_output() {
        assert_eq!(
            dem("_D6corpus11staticArrayFG4iZQe"),
            "corpus.staticArray(int[4])"
        );
        assert_eq!(
            dem("_D6corpus20takesFunctionPointerFPFiZiZv"),
            "corpus.takesFunctionPointer(int function(int))"
        );
        assert_eq!(
            dem("_D6corpus14nestedCompoundFHAyaAxPiZv"),
            "corpus.nestedCompound(const(int*)[][immutable(char)[]])"
        );
        assert_eq!(
            dem("_D6corpus10assocArrayFHAyaiZQg"),
            "corpus.assocArray(int[immutable(char)[]])"
        );
        assert_eq!(
            dem("_D6corpus5Outer6Middle5Inner12deeplyNestedMFZv"),
            "corpus.Outer.Middle.Inner.deeplyNested()"
        );
    }

    #[test]
    fn compound_types_as_values() {
        // A variable of function-pointer type. `P` over a function type is
        // already the pointer spelling, so no trailing `*` is added.
        assert_eq!(dem("_D6module2fpPFiZi"), "int function(int) module.fp");
        // Delegate parameter.
        assert_eq!(
            dem("_D6module3fooFDFiZvZv"),
            "module.foo(void delegate(int))"
        );
    }

    #[test]
    fn detection_predicate() {
        assert!(is_maybe_dlang("_Dmain"));
        assert!(is_maybe_dlang("_D6module4funcFZv"));
        assert!(is_maybe_dlang("_D8demangle3ABCQe1ai"));
        assert!(!is_maybe_dlang("_DDD"));
        assert!(!is_maybe_dlang("_DEBUG"));
        assert!(!is_maybe_dlang("_ZN3foo3barEv"));
        assert!(!is_maybe_dlang("_D"));
    }
}
