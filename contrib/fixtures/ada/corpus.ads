--  Ada corpus fixture (package spec).
--
--  Each declaration pins down one encoding the `ada` backend claims to
--  handle. GNAT emits the mangled names; `nm` reads them back as ground
--  truth.
--
--  Encodings exercised here: package/child nesting (pkg__child__proc),
--  operator subprograms (Oeq/Oadd/...), overloads (the __<digits> suffix),
--  task bodies (the trailing B marker), and non-ASCII identifiers (the
--  U<hex>/W<hex> escapes, which Ada 2012 permits and which no hand-written
--  test in the crate currently derives from a real compiler).

package Corpus is

   type Value is record
      Data : Integer;
   end record;

   --  Plain library-level subprograms.
   procedure Simple;
   function Compute (X : Integer) return Integer;

   --  Operator subprograms -> O<name> encoding.
   function "=" (L, R : Value) return Boolean;
   function "+" (L, R : Value) return Value;
   function "-" (L, R : Value) return Value;
   function "*" (L, R : Value) return Value;
   function "<" (L, R : Value) return Boolean;
   function "<=" (L, R : Value) return Boolean;
   function "&" (L, R : Value) return Value;
   function "**" (L : Value; R : Natural) return Value;
   function "abs" (L : Value) return Value;

   --  Overloaded names -> the trailing __<digits> disambiguator.
   procedure Overloaded (X : Integer);
   procedure Overloaded (X : Float);
   procedure Overloaded (X : Boolean);

   --  A task type: task bodies carry the trailing `B` marker.
   task type Worker is
      entry Start;
   end Worker;

   --  Identifiers ending in digits, and one containing them, to check that
   --  nothing mistakes them for an overload suffix.
   procedure Step_2;
   procedure Phase3;

end Corpus;
