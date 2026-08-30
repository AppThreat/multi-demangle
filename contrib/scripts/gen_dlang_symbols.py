#!/usr/bin/env python3
"""Grammar-aware D mangled-name generator for differential fuzzing (Plan 05).

Generates mangled names from the grammar in the D ABI spec
(https://dlang.org/spec/abi.html#name_mangling) — deliberately not from
`src/dlang.rs`'s parser structure, so a wrong belief shared by parser and
generator cannot manufacture agreement (the Plan 07 circularity).

Output correctness is judged by the oracle (`c++filt -s dlang` inside the
contrib/ toolchain image, libiberty's independent implementation): symbols
the oracle itself rejects are reported separately as generator noise, which
is what keeps this script honest about its own grammar coverage.

Back references follow the ABI's byte-offset encoding, matching libiberty's
`dlang_decode_backref`: `Q` + base-26 number where higher digits are
uppercase letters and the final digit is lowercase, counting the byte
distance from the `Q` back to the start of the referenced identifier (a
digit) or non-basic type (a letter). A distance of zero is invalid.

Usage:
    python3 gen_dlang_symbols.py [--count N] [--seed S]
"""

from __future__ import annotations

import argparse
import random
import string
import sys

MAX_LEN = 400  # skip a draw if the symbol grows past this
MAX_DEPTH = 6

BASIC_TYPES = "nvbhgstiklmfdeopjqcrauw"

TYPE_MODIFIERS = ["x", "Ng", "Ngx", "O", "Ox", "ONg", "ONgx", "y"]

CALL_KINDS = [
    ("F", "D linkage"),
    ("U", "C linkage"),
    ("W", "Windows linkage"),
    ("V", "Pascal linkage"),
    ("R", "C++ linkage"),
    ("Y", "Objective-C linkage"),
]

FUNC_ATTRS = ["Na", "Nb", "Nc", "Nd", "Ni", "Nj", "Nl", "Ne", "Nf", "Nm"]

IDENT_CHARS = string.ascii_letters + "_"


class Skip(Exception):
    """Raised when a draw would overflow the length budget; the symbol is
    discarded and a fresh one is drawn."""


class Gen:
    def __init__(self, rng: random.Random):
        self.rng = rng
        self.buf: list[str] = []
        self.length = 0
        # Byte offsets where an identifier (digit-first) or a non-basic type
        # (letter-first) started — legal back-reference targets.
        self.id_offsets: list[int] = []
        self.type_offsets: list[int] = []

    # -- emission -----------------------------------------------------------

    def emit(self, text: str) -> None:
        self.length += len(text)
        if self.length > MAX_LEN:
            raise Skip()
        self.buf.append(text)

    def here(self) -> int:
        return self.length

    def result(self) -> str:
        return "".join(self.buf)

    def chance(self, p: float) -> bool:
        return self.rng.random() < p

    def pick(self, seq):
        return self.rng.choice(seq)

    # -- names --------------------------------------------------------------

    def name_text(self) -> str:
        n = self.rng.randint(1, 10)
        first = self.rng.choice(IDENT_CHARS)
        rest = "".join(self.rng.choice(IDENT_CHARS + string.digits) for _ in range(n - 1))
        return first + rest

    def lname(self, record: bool = True) -> None:
        text = self.name_text()
        if record:
            self.id_offsets.append(self.here())
        self.emit(f"{len(text)}{text}")

    def backref_encode(self, q: int, target: int) -> None:
        # libiberty measures the distance from the Q's own byte offset
        # (`*ret = qpos - refpos`), so the target resolves to `q - distance`.
        distance = q - target
        # Zero would reference the Q itself; libiberty rejects it.
        if distance <= 0:
            raise Skip()
        digits: list[int] = []
        while distance > 0:
            digits.append(distance % 26)
            distance //= 26
        digits.reverse()
        encoded = "".join(
            chr(ord("A") + d) for d in digits[:-1]
        ) + chr(ord("a") + digits[-1])
        self.emit(encoded)

    def identifier_backref(self) -> None:
        if not self.id_offsets:
            raise Skip()
        target = self.pick(self.id_offsets)
        q = self.here()
        self.emit("Q")
        self.backref_encode(q, target)

    # -- qualified names ----------------------------------------------------

    def qualified_name(self, depth: int, allow_function: bool) -> None:
        parts = self.rng.randint(1, 4 if depth > 0 else 1)
        for part in range(parts):
            last = part == parts - 1
            if last and allow_function and self.chance(0.75):
                self.symbol_function_name(depth)
            else:
                self.symbol_name(depth, allow_template=not last)

    def symbol_function_name(self, depth: int) -> None:
        # A template instance is never the symbol's final function name.
        before = len(self.buf)
        self.symbol_name(depth, allow_template=False)
        if self.buf[before:] == ["0"]:
            return  # an anonymous name carries no function head
        if self.chance(0.35):
            # Member function: `M` + optional type modifiers + function head.
            self.emit("M")
            if self.chance(0.3):
                self.emit(self.pick(TYPE_MODIFIERS))
            self.type_function_no_return(depth)
        else:
            self.type_function_no_return(depth)

    def symbol_name(self, depth: int, allow_template: bool = True) -> None:
        roll = self.rng.random()
        if roll < 0.62:
            self.lname()
        elif roll < 0.72 and allow_template:
            self.template_instance(depth)
        elif roll < 0.82 and self.id_offsets:
            self.identifier_backref()
        elif roll < 0.86 and self.id_offsets:
            # Anonymous names appear after a real parent in practice; the
            # oracle rejects a leading `0`.
            self.emit("0")  # anonymous, also a legal backref target
            self.id_offsets.append(self.here() - 1)
        else:
            self.lname()

    def template_instance(self, depth: int) -> None:
        if depth <= 0:
            raise Skip()
        # Real compilers length-prefix the whole `__T…Z` span when the
        # instance sits inside a qualified name (corpus shape
        # `13__T4tempTiTkZ`). Generate the span in a sub-buffer, then emit
        # the length and the span; the span's back-reference targets rebase
        # past the wrapper digits, while targets from before the instance
        # keep their absolute offsets.
        saved_buf, saved_len = self.buf, self.length
        saved_ids, saved_types = self.id_offsets, self.type_offsets
        self.buf, self.length = [], 0
        # Back-reference targets recorded inside the span are span-relative;
        # outer targets would mix coordinate systems, so the span only
        # back-references within itself.
        self.id_offsets, self.type_offsets = [], []
        self.emit("__T")
        self.lname()
        for _ in range(self.rng.randint(1, 4)):
            self.template_arg(depth - 1)
        self.emit("Z")
        span = "".join(self.buf)
        span_len = self.length
        if span_len > 220:
            raise Skip()
        inner_ids, inner_types = self.id_offsets, self.type_offsets

        self.buf, self.length = saved_buf, saved_len
        self.id_offsets, self.type_offsets = saved_ids, saved_types
        self.emit(str(span_len))
        base = self.here()
        self.buf.append(span)
        self.length += span_len
        self.id_offsets += [base + off for off in inner_ids]
        self.type_offsets += [base + off for off in inner_types]

    def template_arg(self, depth: int) -> None:
        roll = self.rng.random()
        if roll < 0.62:
            self.emit("T")
            self.type(depth - 1)
        elif roll < 0.87:
            self.emit("V")
            kind = self.rng.random()
            if kind < 0.7:
                # Integral parameter, integral value.
                self.emit(self.pick(["h", "g", "s", "t", "i", "k", "l", "m"]))
                self.emit(f"i{self.rng.randint(0, 5000)}")
            else:
                # Character/string parameter, string-literal value
                # (CharWidth Number `_` HexDigits).
                self.emit(self.pick(["a", "u", "w"]))
                chars = self.rng.randint(1, 8)
                text = "".join(
                    self.rng.choice(string.ascii_letters + string.digits)
                    for _ in range(chars)
                )
                self.emit(f"{chars}_" + "".join(f"{ord(c):02x}" for c in text))
        else:
            self.emit("S")
            self.qualified_name(depth - 1, allow_function=False)

    def value(self, depth: int) -> None:
        roll = self.rng.random()
        if roll < 0.45:
            self.emit(str(self.rng.randint(0, 5000)))
        elif roll < 0.7:
            self.emit(f"N{self.rng.randint(1, 5000)}")
        elif roll < 0.75:
            self.emit("n")
        elif roll < 0.9:
            # String literal: CharWidth Number `_` HexDigits, two hex digits
            # per character.
            width = self.pick(["a", "u", "w"])
            chars = self.rng.randint(1, 8)
            text = "".join(
                self.rng.choice(string.printable[:62]) for _ in range(chars)
            )
            hex_digits = "".join(f"{ord(c):02x}" for c in text)
            self.emit(f"{width}{chars}_{hex_digits}")
        else:
            self.emit(f"i{self.rng.randint(0, 9)}")

    # -- types --------------------------------------------------------------

    def type(self, depth: int, no_member: bool = False) -> None:
        if depth <= 0:
            # Leaf: a basic type, always valid at any depth.
            self.emit(self.pick(BASIC_TYPES))
            return
        if self.chance(0.08) and self.type_offsets:
            target = self.pick(self.type_offsets)
            q = self.here()
            self.emit("Q")
            self.backref_encode(q, target)
            return
        roll = self.rng.random()
        if roll < 0.48:
            self.emit(self.pick(BASIC_TYPES))
        elif roll < 0.56:
            self.emit(self.pick(TYPE_MODIFIERS))
            # A modifier cannot precede the `M` member-function marker.
            self.type(depth - 1, no_member=True)
        elif roll < 0.8 and not no_member:
            self.modified_type(depth)
        elif roll < 0.8:
            self.emit("A")  # stand-in where `M` was barred
            self.type(depth - 1)
        else:
            # Non-basic named types are the back-reference pool.
            # NB: the grammar's `I` (identifier) type is rejected by the
            # oracle in every variable/function-type slot, so it is not
            # generated.
            start = self.here()
            self.emit(self.pick(["C", "S", "E", "T"]))
            self.qualified_name(depth - 1, allow_function=False)
            self.type_offsets.append(start)

    def modified_type(self, depth: int) -> None:
        start = self.here()
        roll = self.rng.random()
        if roll < 0.25:
            self.emit("A")  # dynamic array
            self.type(depth - 1)
        elif roll < 0.45:
            self.emit("P")  # pointer
            self.type(depth - 1)
        elif roll < 0.55:
            self.emit("G")  # static array: dimension first
            self.emit(str(self.rng.randint(1, 99)))
            self.type(depth - 1)
        elif roll < 0.62:
            self.emit("H")  # associative array
            self.type(depth - 1)
            self.type(depth - 1)
        elif roll < 0.72:
            self.emit("D")  # delegate
            if self.chance(0.3):
                self.emit(self.pick(TYPE_MODIFIERS))
            self.type_function(depth)
        elif roll < 0.8:
            # Member-function types carry a function type, not any type.
            self.emit("M")
            self.type_function(depth)
        elif roll < 0.9:
            self.emit("Nh")  # vector
            self.type(depth - 1)
        else:
            self.emit("Nn")  # noreturn
        self.type_offsets.append(start)

    def type_function(self, depth: int) -> None:
        self.type_function_no_return(depth)
        self.type(depth)

    def type_function_no_return(self, depth: int) -> None:
        self.emit(self.pick(CALL_KINDS)[0])
        if self.chance(0.35):
            # Func attributes, emitted in the spec's fixed relative order.
            count = self.rng.randint(1, 3)
            attrs = sorted(
                self.rng.sample(FUNC_ATTRS, count),
                key=FUNC_ATTRS.index,
            )
            self.emit("".join(attrs))
        if self.chance(0.9):
            for _ in range(self.rng.randint(1, 4)):
                self.parameter(depth)
        self.emit(self.pick(["X", "Y", "Z"]))

    def parameter(self, depth: int) -> None:
        if self.chance(0.12):
            self.emit("M")  # scope
        if self.chance(0.12):
            self.emit("Nk")  # return
        if self.chance(0.2):
            self.emit(self.pick(["I", "J", "K", "L"]))  # in/out/ref/lazy
        self.type(depth - 1)

    # -- top level ----------------------------------------------------------

    def mangled_name(self) -> str:
        while True:
            try:
                self.buf.clear()
                self.length = 0
                self.id_offsets.clear()
                self.type_offsets.clear()
                if self.chance(0.02):
                    self.emit("_Dmain")
                    return self.result()
                self.emit("_D")
                self.qualified_name(MAX_DEPTH, allow_function=True)
                if self.chance(0.12):
                    self.emit("Z")  # internal symbol: no type
                else:
                    # The trailing type is the variable's type or the
                    # function's return type.
                    self.type(MAX_DEPTH)
                return self.result()
            except Skip:
                continue


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--count", type=int, default=20000)
    parser.add_argument("--seed", type=int, default=1)
    args = parser.parse_args()

    rng = random.Random(args.seed)
    gen = Gen(rng)
    seen = set()
    written = 0
    out = sys.stdout
    while written < args.count:
        symbol = gen.mangled_name()
        if symbol in seen:
            continue
        seen.add(symbol)
        out.write(symbol + "\n")
        written += 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
