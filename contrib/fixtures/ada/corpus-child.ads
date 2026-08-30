--  A child package: GNAT flattens `Corpus.Child.Deep_Proc` into
--  `corpus__child__deep_proc`, which is the multi-component path the
--  backend's namespace walking depends on.

package Corpus.Child is

   procedure Deep_Proc;
   function Nested_Compute (X : Integer) return Integer;

end Corpus.Child;
