(*  Title:      isabellum/backend/Isabelle2025/ML/isabellum_prelude.ML

Loaded into the prover at startup (Isabelle_Process use_prelude). A query
operation that runs a diagnostic Isar command, such as quickcheck, nitpick,
try0 or solve_direct, on the proof state at any position, without editing
the theory. Only Pure's Outer_Syntax and Toplevel are used here: the command
is parsed with the keywords of the theory at that position.
*)

val _ =
  Query_Operation.register {name = "isabellum_command", pri = 0}
    (fn {state = st, args, writeln_result, ...} =>
      let
        val text = (case args of [t] => t | _ => error "Expected one command");
        val thy = Toplevel.theory_of st;
        (* At the print function's own position, so the command's messages carry
           its execution id and are reported with the query *)
        val trs = Outer_Syntax.parse_text thy (K thy) (Position.thread_data ()) text;
        (* A proof that closes a theorem PIDE has already registered (its proof forked at
           the statement) ends with a duplicate fact: the proof itself succeeded *)
        val _ = (fold (Toplevel.command_exception true) trs st; ())
          handle exn =>
            if Exn.is_interrupt exn then Exn.reraise exn
            else if String.isSubstring "Duplicate fact declaration" (Runtime.exn_message exn) then ()
            else Exn.reraise exn;
      in writeln_result "" end);

(* The proof state at a position, in parts for a structured display: the chained facts, then
   for each subgoal its fixed variables (with types), assumptions and conclusion, each pretty
   printed at the display's width. Every part is one result, headed by a line
   "#isabellum:KIND N" (KIND text, fact, count, param, prem, concl or nogoal; N the subgoal) *)
val _ =
  Query_Operation.register {name = "isabellum_goals", pri = 0}
    (fn {state = st, args, writeln_result, ...} =>
      let
        val (margin, types) =
          (case args of
            [m, t] => (the_default 76 (Int.fromString m), t = "true")
          | _ => (76, false));
        fun out kind n prt =
          writeln_result ("#isabellum:" ^ kind ^ " " ^ string_of_int n ^ "\n" ^
            Pretty.string_of_ops (Pretty.output_ops (SOME margin)) prt);
        val state =
          if Toplevel.is_proof st then Toplevel.proof_of st else error "Not inside a proof";
        val ctxt = Proof.context_of state |> Config.put show_types types;
        (* The whole state as Isabelle prints it, with the same settings, for the Text view *)
        val _ = out "text" 0 (Pretty.chunks (Proof.pretty_state (Proof.map_context (K ctxt) state)));
        val goal = SOME (Proof.goal state) handle ERROR _ => NONE;
        val facts =
          (case goal of
            SOME {facts, ...} => facts
          | NONE => (Proof.the_facts state handle ERROR _ => []));
        val _ = facts |> List.app (fn th => out "fact" 0 (Thm.pretty_thm ctxt th));
        fun subgoal (i, sg) =
          let
            val params = Logic.strip_params sg;
            val (names, ctxt') = Variable.variant_fixes (map #1 params) ctxt;
            val frees = map2 (fn x => fn (_, T) => Free (x, T)) names params;
            val inst = curry subst_bounds (rev frees);
            val typed = Config.put show_types true ctxt';
          in
            frees |> List.app (fn v => out "param" i (Syntax.pretty_term typed v));
            Logic.strip_assums_hyp sg |> List.app (fn h => out "prem" i (Syntax.pretty_term ctxt' (inst h)));
            out "concl" i (Syntax.pretty_term ctxt' (inst (Logic.strip_assums_concl sg)))
          end;
      in
        (case goal of
          NONE => out "nogoal" 0 (Pretty.str "")
        | SOME {goal = th, ...} =>
            let val subgoals = Thm.prems_of th in
              out "count" (length subgoals) (Pretty.str "");
              List.app subgoal (map_index (fn (i, sg) => (i + 1, sg)) subgoals)
            end)
      end);

(* What a name means at a position, for a theory the prover has not checked in this session (a
   library theory shown in a reference window): one result for each kind of thing the name is,
   headed by a line "#isabellum:entity KIND", then its full name, the file and line defining it
   (empty and 0 when unknown) and what it is *)
val _ =
  Query_Operation.register {name = "isabellum_entity", pri = 0}
    (fn {state = st, args, writeln_result, ...} =>
      let
        val name = (case args of [n] => n | _ => error "Expected one name");
        val ctxt = Toplevel.context_of st;
        val thy = Proof_Context.theory_of ctxt;
        val text = Pretty.string_of_ops (Pretty.output_ops (SOME 80));
        fun look kind space pretty =
          let val full = Name_Space.intern space name in
            if Name_Space.declared space full then
              let
                val {pos, ...} = Name_Space.the_entry space full;
                val file =
                  (case Position.file_of pos of
                    SOME f => (File.platform_path (Path.explode f) handle ERROR _ => f)
                  | NONE => "");
                val line = the_default 0 (Position.line_of pos);
              in
                writeln_result ("#isabellum:entity " ^ kind ^ "\n" ^ full ^ "\n" ^ file ^ "\n" ^
                  string_of_int line ^ "\n" ^ text (pretty full))
              end
            else ()
          end handle ERROR _ => () | TYPE _ => ();
        val consts = Proof_Context.consts_of ctxt;
      in
        look "constant" (Proof_Context.const_space ctxt) (fn c =>
          Pretty.block [Pretty.str (Proof_Context.extern_const ctxt c), Pretty.str " ::", Pretty.brk 1,
            Syntax.pretty_typ ctxt (Logic.unvarifyT_global (Consts.type_scheme consts c))]);
        look "fact" (Facts.space_of (Proof_Context.facts_of ctxt)) (fn _ =>
          Pretty.chunks (map (Thm.pretty_thm ctxt) (take 6 (Proof_Context.get_thms ctxt name))));
        look "type" (Proof_Context.type_space ctxt) (fn c => Pretty.str ("type " ^ Proof_Context.extern_type ctxt c));
        look "class" (Proof_Context.class_space ctxt) (fn c => Pretty.str ("class " ^ c));
        look "locale" (Locale.locale_space thy) (fn c => Pretty.str ("locale " ^ c));
        writeln_result "#isabellum:done"
      end);
