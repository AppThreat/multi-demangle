--  Main program for the Ada corpus. `_ada_main` is itself a fixture: the
--  `_ada_` prefix marks library-level subprograms, and the GNAT binder
--  generates an `ada_main` unit full of `B_<digits>` anonymous blocks and
--  `__elabb`/`__elabs` elaboration symbols worth collecting.

with Corpus;
with Corpus.Child;

procedure Main is
   A : Corpus.Value := (Data => 1);
   B : Corpus.Value := (Data => 2);
   W : Corpus.Worker;
   use type Corpus.Value;
begin
   Corpus.Simple;
   Corpus.Step_2;
   Corpus.Phase3;
   Corpus.Overloaded (1);
   Corpus.Overloaded (1.0);
   Corpus.Overloaded (True);
   Corpus.Child.Deep_Proc;

   if A = B or else A < B then
      A := A + B;
   end if;

   W.Start;
end Main;
