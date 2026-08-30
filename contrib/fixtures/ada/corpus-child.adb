package body Corpus.Child is

   procedure Deep_Proc is
   begin
      null;
   end Deep_Proc;

   function Nested_Compute (X : Integer) return Integer is
   begin
      return Compute (X) + 1;
   end Nested_Compute;

end Corpus.Child;
