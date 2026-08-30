--  Ada corpus fixture (package body).
--
--  The body adds the constructs that only exist at runtime: a nested
--  declare block (the B_<digits> anonymous-block components) and the task
--  body itself.

package body Corpus is

   procedure Simple is
   begin
      null;
   end Simple;

   function Compute (X : Integer) return Integer is
      Local_Total : Integer := X;
   begin
      --  A named nested block plus an anonymous one: GNAT flattens both
      --  into the symbol path, anonymous ones as B_<digits>.
      Outer_Block :
      declare
         Scratch : Integer := 0;
         procedure Inner is
         begin
            Scratch := Scratch + 1;
         end Inner;
      begin
         Inner;
         Local_Total := Local_Total + Scratch;
      end Outer_Block;

      declare
         procedure Anonymously_Nested is
         begin
            Local_Total := Local_Total * 2;
         end Anonymously_Nested;
      begin
         Anonymously_Nested;
      end;

      return Local_Total;
   end Compute;

   function "=" (L, R : Value) return Boolean is (L.Data = R.Data);
   function "+" (L, R : Value) return Value is (Value'(Data => L.Data + R.Data));
   function "-" (L, R : Value) return Value is (Value'(Data => L.Data - R.Data));
   function "*" (L, R : Value) return Value is (Value'(Data => L.Data * R.Data));
   function "<" (L, R : Value) return Boolean is (L.Data < R.Data);
   function "<=" (L, R : Value) return Boolean is (L.Data <= R.Data);
   function "&" (L, R : Value) return Value is (Value'(Data => L.Data + R.Data));
   function "**" (L : Value; R : Natural) return Value is
     (Value'(Data => L.Data ** R));
   function "abs" (L : Value) return Value is (Value'(Data => abs L.Data));

   procedure Overloaded (X : Integer) is
   begin
      null;
   end Overloaded;

   procedure Overloaded (X : Float) is
   begin
      null;
   end Overloaded;

   procedure Overloaded (X : Boolean) is
   begin
      null;
   end Overloaded;

   task body Worker is
   begin
      accept Start;
   end Worker;

   procedure Step_2 is
   begin
      null;
   end Step_2;

   procedure Phase3 is
   begin
      null;
   end Phase3;

end Corpus;
