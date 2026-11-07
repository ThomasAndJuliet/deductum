/*  Title:      backend/Isabelle2025/src/backend.scala

    "isabelle deductum_backend": one PIDE session for Deductum's session host,
    driven by JSON lines on stdin and answering with JSON lines on stdout.

    The host owns the buffers (design C11); this process holds the prover's copy
    of each opened theory, applies the host's edits in order, and reports the
    prover's view of them: command spans and status, messages, proof states at
    any position, and query operations such as Sledgehammer. Offsets on the wire
    count Unicode code points of the LF-normalized text.

    Derived from Isabelle2025's Tools/VSCode sources (Makarius) and the
    Query_Operation mechanism; distributed under the Isabelle license (BSD).
*/

package deductum


import isabelle._

import java.io.{File => JFile, BufferedReader, FileDescriptor, FileOutputStream,
  InputStreamReader, OutputStream, PrintStream}
import java.nio.charset.StandardCharsets

import scala.util.parsing.input.Reader


object Backend {
  val isabelle_tool =
    Isabelle_Tool("deductum_backend", "PIDE session for the Deductum session host",
      Scala_Project.here,
      { args =>
        var dirs: List[Path] = Nil
        var include_sessions: List[String] = Nil
        var logic = Isabelle_System.getenv("ISABELLE_LOGIC")
        var options = Options.init()
        var no_build = false

        val getopts = Getopts("""
Usage: isabelle deductum_backend [OPTIONS]

  Options are:
    -d DIR       include session directory
    -i NAME      include session in name-space of theories
    -l NAME      logic session name
    -n           no build of session image on startup
    -o OPTION    override Isabelle system OPTION (via NAME=VAL or NAME)

  Run one PIDE session for Deductum, with JSON lines on stdin/stdout.
""",
          "d:" -> (arg => dirs = dirs ::: List(Path.explode(File.standard_path(arg)))),
          "i:" -> (arg => include_sessions = include_sessions ::: List(arg)),
          "l:" -> (arg => logic = arg),
          "n" -> (_ => no_build = true),
          "o:" -> (arg => options = options + arg))

        val more_args = getopts(args)
        if (more_args.nonEmpty) getopts.usage()

        val out = new PrintStream(new FileOutputStream(FileDescriptor.out), false, "UTF-8")
        // Prover chatter on System.out would corrupt the protocol
        System.setOut(new PrintStream(OutputStream.nullOutputStream()))
        new Backend(out, options, logic, dirs, include_sessions, no_build).run()
      })


  /* models: the prover's copy of a file */

  sealed case class Model(
    session: Session,
    node_name: Document.Node.Name,
    text: String,
    opened: Boolean,
    pending_edits: List[Text.Edit] = Nil,
    last_perspective: Document.Node.Perspective_Text.T = Document.Node.Perspective_Text.empty,
    seq: Long = 0
  ) extends Document.Model {
    lazy val bmp: Boolean = text.length == text.codePointCount(0, text.length)
    lazy val cp_length: Int = if (bmp) text.length else text.codePointCount(0, text.length)

    def to16(cp: Int): Int = {
      val i = cp max 0 min cp_length
      if (bmp) i else text.offsetByCodePoints(0, i)
    }
    def from16(i: Int): Int = {
      val j = i max 0 min text.length
      if (bmp) j else text.codePointCount(0, j)
    }

    def is_stable: Boolean = pending_edits.isEmpty
    def get_text(range: Text.Range): Option[String] = range.try_substring(text)
    def node_required: Boolean = opened && is_theory

    def get_blob: Option[Document.Blobs.Item] =
      if (is_theory) None
      else {
        Some(Document.Blobs.Item(Bytes(Symbol.encode(text)), text, Symbol.Text_Chunk(text),
          pending_edits.nonEmpty))
      }

    def untyped_data: AnyRef = File_Format.registry.parse_data(node_name, text)

    def node_header: Document.Node.Header =
      session.resources.special_header(node_name) getOrElse
        session.resources.check_thy(node_name, Scan.char_reader(text))

    def perspective(checking: Boolean, overlays: Document.Node.Overlays)
      : Document.Node.Perspective_Text.T =
    {
      if (!is_theory) Document.Node.Perspective_Text.empty
      else if (checking && opened) {
        Document.Node.Perspective(true, Text.Perspective(List(Text.Range(0, text.length))),
          overlays)
      }
      else Document.Node.Perspective(false, Text.Perspective.empty, Document.Node.Overlays.empty)
    }

    def replace_text(text1: String): Model =
      Text.Edit.replace(0, text, text1) match {
        case Nil => this
        case edits => copy(text = text1, pending_edits = pending_edits ::: edits, seq = seq + 1)
      }

    def edit(start_cp: Int, stop_cp: Int, insert: String): Model = {
      val start = to16(start_cp)
      val stop = to16(stop_cp) max start
      val removed = text.substring(start, stop)
      val edits =
        (if (removed.nonEmpty) List(Text.Edit.remove(start, removed)) else Nil) :::
        (if (insert.nonEmpty) List(Text.Edit.insert(start, insert)) else Nil)
      val text1 = text.substring(0, start) + insert + text.substring(stop)
      copy(text = text1, pending_edits = pending_edits ::: edits, seq = seq + 1)
    }
  }


  /* state */

  sealed case class Query(
    id: String,
    file: JFile,
    command: Command,
    print_function: String,
    args: List[String],
    instance: String,
    status: String = "",
    output: List[XML.Elem] = Nil,
    exec_id: Document_ID.Exec = Document_ID.none)

  sealed case class State(
    models: Map[JFile, Model] = Map.empty,
    overlays: Document.Overlays = Document.Overlays.empty,
    checking: Boolean = true,
    pending_input: Set[JFile] = Set.empty,
    pending_output: Set[JFile] = Set.empty,
    queries: Map[String, Query] = Map.empty
  ) {
    lazy val document_blobs: Document.Blobs =
      Document.Blobs(
        (for {
          (_, model) <- models.iterator
          blob <- model.get_blob
        } yield (model.node_name -> blob)).toMap)

    def pending_edits: Document.Pending_Edits = Document.Pending_Edits.make(models.values)

    def update_models(changed: Iterable[(JFile, Model)]): State =
      copy(
        models = models ++ changed,
        pending_input = pending_input ++ changed.map(_._1),
        pending_output = pending_output ++ changed.map(_._1))
  }


  /* JSON */

  type Obj = Map[String, Any]

  def message_kind(name: String): String =
    if (name.endsWith("_message")) name.stripSuffix("_message") else name

  def sendbacks(body: XML.Body): List[String] =
    body.flatMap {
      case XML.Elem(Markup(Markup.SENDBACK, _), b) => List(XML.content(b))
      case XML.Elem(_, b) => sendbacks(b)
      case XML.Text(_) => Nil
    }

  /* Formatted output with the colours of its markup, as jEdit sets it: the text in
     Isabelle's own symbol encoding, and [start, end, colour] in code points of it */
  /* text, colour spans and hover tips (a term's type, what a name is) of output */

  def styled(body: XML.Body, margin: Double): (String, List[List[Any]]) = {
    val (text, spans, _) = styled_tips(body, margin)
    (text, spans)
  }

  def styled_tips(body: XML.Body, margin: Double): (String, List[List[Any]], List[List[Any]]) = {
    val formatted = Pretty.formatted(body, margin = margin, metric = Symbol.Metric)
    val text = new StringBuilder
    val spans = new scala.collection.mutable.ListBuffer[List[Any]]
    val tips = new scala.collection.mutable.ListBuffer[List[Any]]
    var pos = 0
    def walk(tree: XML.Tree, colour: Option[String]): Unit =
      tree match {
        case XML.Text(s) =>
          val n = s.codePointCount(0, s.length)
          for (c <- colour if c != "main" && n > 0) spans += List(pos, pos + n, c)
          text ++= s
          pos += n
        // Markup carrying a body of its own (the type of a term, for its tooltip): only the
        // text it marks is shown
        case XML.Elem(Markup(XML.XML_ELEM, props), XML.Elem(Markup(XML.XML_BODY, _), tip) :: body) =>
          val start = pos
          body.foreach(walk(_, colour))
          val name = props.collectFirst { case (XML.XML_NAME, n) => n }.getOrElse("")
          if ((name == Markup.TYPING || name == Markup.SORTING) && pos > start)
            tips += List(start, pos, ":: " + Symbol.decode(XML.content(tip)))
        case XML.Elem(markup @ Markup.Entity(kind, name), body) =>
          val start = pos
          val c = Rendering.get_text_color(markup).map(_.toString) orElse colour
          body.foreach(walk(_, c))
          if (kind != "" && pos > start) tips += List(start, pos, Rendering.gui_name(name, kind = kind))
        case XML.Elem(markup, body) =>
          val c = Rendering.get_text_color(markup).map(_.toString) orElse colour
          body.foreach(walk(_, c))
      }
    formatted.foreach(walk(_, None))
    (text.toString, spans.toList, tips.toList)
  }

  def message_json(elem: XML.Elem, margin: Double): Obj = {
    val (raw, spans, tips) = styled_tips(elem.body, margin)
    Map(
      "kind" -> message_kind(elem.name),
      "text" -> Symbol.decode(raw),
      "raw" -> raw,
      "spans" -> spans,
      "tips" -> tips,
      "sendbacks" -> sendbacks(elem.body))
  }

  def command_status(snapshot: Document.Snapshot, command: Command): String = {
    val status =
      Document_Status.Command_Status.merge(
        snapshot.state.command_states(snapshot.version, command).iterator.map(_.document_status))
    if (status.is_running) "running"
    else if (status.is_failed) "failed"
    else if (status.is_canceled) "canceled"
    else if (status.is_warned) "warned"
    else if (status.is_finished) "finished"
    else "unprocessed"
  }
}

class Tools extends Isabelle_Scala_Tools(Backend.isabelle_tool)


class Backend_Resources(
  val options: Options,
  session_background: Sessions.Background,
  backend: Backend
) extends Resources(session_background) {
  def node_file(name: Document.Node.Name): JFile = new JFile(name.node)

  def node_name(file: JFile): Document.Node.Name =
    find_theory(file) getOrElse {
      val node = file.getPath
      val theory = theory_name(Sessions.DRAFT, Thy_Header.theory_name(node))
      if (session_base.loaded_theory(theory)) Document.Node.Name.loaded_theory(theory)
      else Document.Node.Name(node, theory = theory)
    }

  override def migrate_name(standard_name: Document.Node.Name): Document.Node.Name =
    node_name(Path.explode(standard_name.node).canonical_file)

  override def append_path(prefix: String, source_path: Path): String = {
    val path = source_path.expand
    if (prefix == "" || path.is_absolute) File.platform_path(path)
    else if (path.is_current) prefix
    else if (path.is_basic && !prefix.endsWith("/") && !prefix.endsWith(JFile.separator))
      prefix + JFile.separator + File.platform_path(path)
    else if (path.is_basic) prefix + File.platform_path(path)
    else File.absolute(new JFile(prefix + JFile.separator + File.platform_path(path))).getPath
  }

  override def read_dir(dir: String): List[String] =
    File.read_dir(Path.explode(File.standard_path(dir)))

  override def with_thy_reader[A](name: Document.Node.Name, f: Reader[Char] => A): A = {
    val file = node_file(name)
    backend.get_model(file) match {
      case Some(model) => f(Scan.char_reader(model.text))
      case None if file.isFile => using(Scan.byte_reader(file))(f)
      case None => error("No such file: " + quote(file.toString))
    }
  }

  override def commit(change: Session.Change): Unit =
    if (change.deps_changed || undefined_blobs(change.version).nonEmpty) {
      backend.invoke_load()
    }
}


class Backend(
  out: PrintStream,
  options0: Options,
  logic: String,
  dirs: List[Path],
  include_sessions: List[String],
  no_build: Boolean
) {
  backend =>

  import Backend._

  @volatile private var options = options0.bool.update("editor_output_state", true)

  private val state = Synchronized(State())
  @volatile private var _session: Option[Session] = None
  def session: Session = _session getOrElse error("No session")
  def resources: Backend_Resources = session.resources.asInstanceOf[Backend_Resources]

  def get_model(file: JFile): Option[Model] = state.value.models.get(file)


  /* output */

  private val out_lock = new Object

  def emit(obj: Obj): Unit = out_lock.synchronized {
    out.print(JSON.Format(obj))
    out.print("\n")
    out.flush()
  }

  def event(name: String, fields: (String, Any)*): Unit =
    emit(Map[String, Any]("event" -> name) ++ fields)

  def reply(id: Any, fields: (String, Any)*): Unit =
    emit(Map[String, Any]("reply" -> id) ++ fields)

  def log(msg: String): Unit = event("log", "text" -> msg)


  /* input to the prover */

  private val delay_input: Delay =
    Delay.last(Time.seconds(0.05)) { flush_input() }

  private val delay_load: Delay =
    Delay.last(Time.seconds(0.2)) { resolve_dependencies() }

  private val delay_output: Delay =
    Delay.last(Time.seconds(0.3)) { flush_output() }

  def invoke_load(): Unit = delay_load.invoke()

  @volatile private var stopping = false

  private def flush_input(): Unit = if (!stopping) {
    state.change { st =>
      val changed =
        (for {
          file <- st.pending_input.iterator
          model <- st.models.get(file)
          perspective = model.perspective(st.checking, st.overlays(model.node_name))
          if model.pending_edits.nonEmpty || model.last_perspective != perspective
        } yield {
          val edits = model.node_edits(model.node_header, model.pending_edits, perspective)
          (edits, file -> model.copy(pending_edits = Nil, last_perspective = perspective))
        }).toList
      if (changed.nonEmpty) session.update(st.document_blobs, changed.flatMap(_._1))
      st.copy(models = st.models ++ changed.map(_._2), pending_input = Set.empty)
    }
  }

  private def read_file(name: Document.Node.Name): Option[String] =
    resources.make_theory_content(name) orElse {
      try { Some(Line.normalize(File.read(resources.node_file(name)))) }
      catch { case ERROR(_) => None }
    }

  private def resolve_dependencies(): Unit = {
    val (invoke_input, invoke_again) =
      state.change_result { st =>
        val stable_tip_version = session.stable_tip_version(st.models.values)
        val thy_files = resources.resolve_dependencies(st.models.values, Nil)
        val aux_files = stable_tip_version.toList.flatMap(resources.undefined_blobs)
        val loaded =
          (for {
            node_name <- thy_files.iterator ++ aux_files.iterator
            file = resources.node_file(node_name)
            if !st.models.isDefinedAt(file)
            text <- read_file(node_name)
          } yield {
            file -> Model(session, node_name, "", opened = false).replace_text(text)
          }).toList
        ((loaded.nonEmpty, stable_tip_version.isEmpty), st.update_models(loaded))
      }
    if (invoke_input) delay_input.invoke()
    if (invoke_again) delay_load.invoke()
  }


  /* output to the host */

  private def node_json(model: Model, snapshot: Document.Snapshot): Obj = {
    val status = Document_Status.Node_Status.make(snapshot.state, snapshot.version, model.node_name)
    val base: Obj =
      Map("event" -> "node", "file" -> model.node_name.node, "theory" -> model.node_name.theory,
        "seq" -> model.seq, "status" -> status.json,
        // A bad import or header, which leaves the theory and its importers waiting
        "header_errors" -> snapshot.node.header.errors.map(e => Symbol.decode(e)),
        // The theories it imports, which are checked first
        "imports" -> snapshot.node.header.imports.map(_.node).filter(_.nonEmpty))
    if (!model.opened) base
    else {
      note_tool_execs(snapshot)
      val commands =
        (for {
          (command, start) <- snapshot.node.command_iterator()
          if !command.is_ignored
        } yield List(model.from16(start), model.from16(start + command.length),
          command.span.name, command_status(snapshot, command))).toList
      val rendering = new Rendering(snapshot, options, session)
      // The colours of the prover's markup over the text: identifiers, keywords, types
      val colours =
        snapshot.cumulate[Option[Rendering.Color.Value]](
          Text.Range(0, model.text.length), None, Rendering.text_color_elements,
          _ => { case (_, Text.Info(_, elem)) => Rendering.get_text_color(elem.markup).map(Some(_)) })
      val colour_spans = {
        val out = new scala.collection.mutable.ListBuffer[List[Any]]
        for (Text.Info(range, c) <- colours; colour <- c if colour != Rendering.Color.main) {
          val s = model.from16(range.start)
          val e = model.from16(range.stop)
          val name = colour.toString
          out.lastOption match {
            case Some(List(ps: Int, pe: Int, pn: String)) if pe == s && pn == name =>
              out(out.length - 1) = List(ps, e, name)
            case _ => out += List(s, e, name)
          }
        }
        out.toList
      }
      val messages =
        (for {
          Text.Info(range, elem) <- rendering.text_messages(Text.Range(0, model.text.length))
          kind = message_kind(elem.name)
          if kind == Markup.ERROR || kind == Markup.WARNING || kind == Markup.LEGACY ||
            kind == Markup.INFORMATION
        } yield {
          message_json(elem, 100.0) ++
            Map("start" -> model.from16(range.start), "end" -> model.from16(range.stop))
        }).take(2000)
      base ++ Map("commands" -> commands, "messages" -> messages, "colours" -> colour_spans)
    }
  }

  private def flush_output(): Unit = {
    val st = state.value
    val pending = st.pending_edits
    val results =
      for {
        file <- st.pending_output.toList
        model <- st.models.get(file)
      } yield {
        val snapshot = session.snapshot(model.node_name, pending)
        if (snapshot.is_outdated) Left(file)
        else Right((file, node_json(model, snapshot)))
      }
    val postponed = results.collect { case Left(file) => file }.toSet
    val flushed = results.collect { case Right((file, _)) => file }.toSet
    for (case Right((_, json)) <- results) emit(json)
    state.change(st1 => st1.copy(pending_output = (st1.pending_output -- flushed) ++ postponed))
    if (postponed.nonEmpty) delay_output.invoke()
  }

  private val prover_output =
    Session.Consumer[Session.Commands_Changed](getClass.getName) {
      case changed =>
        val st = state.value
        val files = changed.nodes.toList.map(resources.node_file).filter(st.models.isDefinedAt)
        if (files.nonEmpty) {
          state.change(st1 => st1.copy(pending_output = st1.pending_output ++ files))
          delay_output.invoke()
        }
        if (st.queries.nonEmpty) update_queries(changed.nodes)
    }

  private val phase_output =
    Session.Consumer[Session.Phase](getClass.getName) {
      case phase =>
        event("phase", "phase" -> phase.print)
    }

  private val syslog_output =
    Session.Consumer[Prover.Output](getClass.getName) {
      case output => log(XML.content(output.message))
    }


  /* requests */

  private def file_of(obj: Obj): JFile =
    new JFile(JSON.string(obj, "file") getOrElse error("Missing file"))

  def open(file: JFile, text: String, rev: Option[Long]): Unit = {
    state.change { st =>
      val model0 =
        st.models.getOrElse(file, Model(session, resources.node_name(file), "", opened = true))
      val model1a = model0.copy(opened = true).replace_text(Line.normalize(text))
      val model1 = rev.fold(model1a)(r => model1a.copy(seq = r))
      st.update_models(List(file -> model1))
    }
    delay_input.invoke()
    delay_load.invoke()
  }

  def edit(file: JFile, edits: List[(Int, Int, String)], rev: Option[Long]): Unit = {
    state.change { st =>
      st.models.get(file) match {
        case Some(model) =>
          val model1a = edits.foldLeft(model) { case (m, (start, stop, text)) =>
            m.edit(start, stop, Line.normalize(text)) }
          val model1 = rev.fold(model1a)(r => model1a.copy(seq = r))
          st.update_models(List(file -> model1))
        case None => error("File not open: " + file)
      }
    }
    delay_input.invoke()
  }

  def close(file: JFile): Unit = {
    state.change { st =>
      st.models.get(file) match {
        case Some(model) =>
          val text = read_file(model.node_name) getOrElse model.text
          st.update_models(List(file -> model.copy(opened = false).replace_text(text)))
        case None => st
      }
    }
    delay_input.invoke()
  }

  def disk_changed(files: List[JFile]): Unit = {
    state.change { st =>
      val changed =
        for {
          file <- files
          model <- st.models.get(file)
          if !model.opened
          text <- read_file(model.node_name)
          if text != model.text
        } yield file -> model.replace_text(text)
      st.update_models(changed)
    }
    delay_input.invoke()
  }

  /* Proof tools typed in the text (sledgehammer, nitpick, …), running: they alone are stopped */
  val tools = Set("sledgehammer", "try", "try0", "nitpick", "quickcheck", "solve_direct", "nunchaku")

  /* The executions of proof tools seen running. Stop cancels these too: while the prover is
     busy, the current version's executions may not be assigned yet, and a tool would then be
     found running but not cancelled. */
  private val tool_execs = Synchronized(Set.empty[Document_ID.Exec])

  private def note_tool_execs(snapshot: Document.Snapshot): Unit = {
    val running =
      snapshot.node.commands.iterator.filter(command =>
        tools(command.span.name) && command_status(snapshot, command) == "running").toList
    if (running.nonEmpty) {
      assignment(snapshot).foreach(a =>
        tool_execs.change(_ ++ running.flatMap(command => a.command_execs.getOrElse(command.id, Nil))))
    }
  }

  /* The executions assigned to a snapshot's version, or else to the last stable one (its
     unchanged commands are the same) */
  private def assignment(snapshot: Document.Snapshot): Option[Document.State.Assignment] =
    try { Some(snapshot.state.the_assignment(snapshot.version)) }
    catch {
      case _: Document.State.Fail =>
        try { Some(snapshot.state.the_assignment(snapshot.state.recent_stable.version.get_finished)) }
        catch { case exn: Throwable if !Exn.is_interrupt(exn) => None }
    }

  def cancel_tools(): Int = {
    val seen = tool_execs.change_result(execs => (execs.toList, Set.empty))
    val execs = (running_execs(state.value, cmd => tools(cmd.span.name)) ++ seen).distinct
    execs.foreach(session.cancel_exec)
    execs.length
  }

  def running_execs(st: State, only: Command => Boolean = _ => true): List[Document_ID.Exec] = {
    val pending = st.pending_edits
    (for {
      model <- st.models.valuesIterator
      if model.is_theory
      snapshot = session.snapshot(model.node_name, pending)
      assignment <- assignment(snapshot).iterator
      command <- snapshot.node.commands.iterator
      if only(command) && command_status(snapshot, command) == "running"
      exec_id <- assignment.command_execs.getOrElse(command.id, Nil)
    } yield exec_id).toList
  }

  def cancel(): Int = {
    val execs = running_execs(state.value)
    execs.foreach(session.cancel_exec)
    execs.length
  }

  def set_checking(on: Boolean): Unit = {
    state.change(st =>
      st.copy(checking = on, pending_input = st.pending_input ++ st.models.keys))
    if (!on) cancel()
    else retouch_canceled()
    delay_input.invoke()
  }

  /* Interrupted commands keep their failed execs across perspective changes; an edit
     that removes and restores a command's last character makes the prover run them again. */
  private def retouch_canceled(): Unit = {
    state.change { st =>
      val pending = st.pending_edits
      val changed =
        for {
          (file, model) <- st.models.toList
          if model.opened && model.is_theory
          snapshot = session.snapshot(model.node_name, pending)
          if !snapshot.is_outdated
          starts =
            (for {
              (command, start) <- snapshot.node.command_iterator()
              if command.length > 0 && command_status(snapshot, command) == "canceled"
            } yield start + command.length - 1).toList
          if starts.nonEmpty
        } yield {
          val edits =
            starts.reverse.flatMap { i =>
              val c = model.text.substring(i, i + 1)
              List(Text.Edit.remove(i, c), Text.Edit.insert(i, c))
            }
          file -> model.copy(pending_edits = model.pending_edits ::: edits)
        }
      st.update_models(changed)
    }
  }

  def state_query(id: Any, file: JFile, offset: Int, margin: Double): Unit = {
    val st = state.value
    st.models.get(file) match {
      case None => reply(id, "error" -> ("File not open: " + file))
      case Some(model) =>
        val snapshot = session.snapshot(model.node_name, st.pending_edits)
        snapshot.current_command(model.node_name, model.to16(offset)) match {
          case None =>
            reply(id, "command" -> null, "messages" -> Nil, "outdated" -> snapshot.is_outdated)
          case Some(command) =>
            val start = snapshot.node.command_start(command).map(snapshot.convert) getOrElse 0
            val messages =
              snapshot.command_results(command).iterator.map(_._2).toList
                .filter(elem => Markup.messages.contains(message_kind(elem.name)))
                .map(message_json(_, margin))
            reply(id,
              "command" -> Map(
                "start" -> model.from16(start),
                "end" -> model.from16(start + command.length),
                "name" -> command.span.name,
                "status" -> command_status(snapshot, command)),
              "messages" -> messages,
              "outdated" -> snapshot.is_outdated)
        }
    }
  }

  def start_query(id: String, file: JFile, offset: Int, operation: String, args: List[String])
    : Unit =
  {
    val st = state.value
    val command =
      for {
        model <- st.models.get(file)
        snapshot = session.snapshot(model.node_name, st.pending_edits)
        command <- snapshot.current_command(model.node_name, model.to16(offset))
      } yield (model, snapshot, command)
    // On a placeholder proof, query the goal before it, and offer to replace it
    def placeholder(command: Command): Boolean =
      command.span.name == "sorry" || command.span.name == "oops"
    def command_range(snapshot: Document.Snapshot, model: Model, command: Command): Obj = {
      val start = snapshot.node.command_start(command).map(snapshot.convert) getOrElse 0
      Map("start" -> model.from16(start), "end" -> model.from16(start + command.length))
    }
    val target =
      command.map { case (model, snapshot, command) =>
        if (placeholder(command)) {
          snapshot.node.commands.reverse.iterator(command).drop(1)
            .find(cmd => !cmd.is_ignored) match {
            case Some(before) => (model, snapshot, before, Some(command_range(snapshot, model, command)))
            case None => (model, snapshot, command, None)
          }
        }
        else (model, snapshot, command, None)
      }
    target match {
      case None => event("query", "query" -> id, "status" -> "failed", "error" -> "No command here")
      case Some((model, snapshot, command, replace)) =>
        val instance = Document_ID.make().toString
        val query = Query(id, file, command, operation + "_query", args, instance)
        state.change(st1 =>
          st1.copy(
            queries = st1.queries + (id -> query),
            overlays = st1.overlays.insert(command, query.print_function, instance :: args),
            pending_input = st1.pending_input + file))
        val range = command_range(snapshot, model, command)
        event("query", "query" -> id, "status" -> "waiting",
          "start" -> range("start"), "end" -> range("end"), "command" -> command.span.name,
          "replace" -> replace.orNull, "messages" -> Nil)
        delay_input.invoke()
    }
  }

  private def finish_query(query: Query): Unit = {
    state.change(st =>
      st.copy(
        queries = st.queries - query.id,
        overlays = st.overlays.remove(query.command, query.print_function,
          query.instance :: query.args),
        pending_input = st.pending_input + query.file))
    delay_input.invoke()
  }

  def cancel_query(id: String): Unit = {
    state.value.queries.get(id) match {
      case Some(query) =>
        if (query.exec_id != Document_ID.none) session.cancel_exec(query.exec_id)
        finish_query(query)
        event("query", "query" -> id, "status" -> "canceled", "messages" ->
          query.output.map(message_json(_, 100.0)))
      case None =>
    }
  }

  private def update_queries(nodes: Set[Document.Node.Name]): Unit = {
    for (query <- state.value.queries.values if nodes(query.command.node_name)) {
      val snapshot = session.snapshot(query.command.node_name, state.value.pending_edits)
      val command_results = snapshot.command_results(query.command)
      val results =
        (for {
          case (_, elem @ XML.Elem(Markup(Markup.RESULT, props), _)) <- command_results.iterator
          if props.contains((Markup.INSTANCE, query.instance))
        } yield elem).toList
      val removed = !snapshot.get_node(query.command.node_name).commands.contains(query.command)
      val exec_id0 =
        results.collectFirst {
          case XML.Elem(Markup(_, Position.Id(id)), List(elem: XML.Elem))
          if elem.name == Markup.RUNNING => id
        } getOrElse query.exec_id
      // Results the operation reported, then the plain messages of its execution
      // (a command run by deductum_command prints with writeln)
      val output =
        (for {
          case XML.Elem(_, List(XML.Elem(markup, body))) <- results
          if Markup.messages.contains(markup.name) && XML.content(body).nonEmpty
        } yield XML.Elem(Markup(markup.name, markup.properties), body)) :::
        (if (exec_id0 == Document_ID.none) Nil
         else
          (for {
            case (_, elem @ XML.Elem(Markup(name, props), _)) <- command_results.iterator
            if name != Markup.RESULT && Markup.messages.values.exists(_ == name) &&
              (props match { case Position.Id(id) => id == exec_id0 case _ => false })
          } yield elem).toList)
      def has(name: String): Boolean =
        results.exists { case XML.Elem(_, List(elem: XML.Elem)) => elem.name == name case _ => false }
      val status =
        if (removed || has(Markup.FINISHED)) "finished"
        else if (has(Markup.RUNNING)) "running"
        else "waiting"
      val exec_id =
        results.collectFirst {
          case XML.Elem(Markup(_, Position.Id(id)), List(elem: XML.Elem))
          if elem.name == Markup.RUNNING => id
        } getOrElse query.exec_id
      if ((output != query.output || status != query.status) && !snapshot.is_outdated) {
        state.change(st =>
          st.copy(queries =
            if (st.queries.isDefinedAt(query.id))
              st.queries + (query.id -> query.copy(output = output, status = status,
                exec_id = exec_id))
            else st.queries))
        event("query", "query" -> query.id, "status" -> status,
          "messages" -> output.map(message_json(_, 100.0)))
        if (status == "finished") finish_query(query)
      }
      else if (exec_id != query.exec_id) {
        state.change(st =>
          st.copy(queries =
            st.queries.get(query.id).fold(st.queries)(q =>
              st.queries + (query.id -> q.copy(exec_id = exec_id)))))
      }
    }
  }

  def symbols_json: List[Obj] =
    for (entry <- Symbol.symbols.entries) yield {
      Map("symbol" -> entry.symbol, "name" -> entry.name,
        "code" -> entry.code.getOrElse(0), "abbrevs" -> entry.abbrevs, "groups" -> entry.groups,
        "argument" -> entry.argument.toString)
    }

  def syntax_json(file: Option[JFile]): Obj = {
    val syntax =
      file.flatMap(get_model) match {
        case Some(model) => session.recent_syntax(model.node_name)
        case None => resources.session_base.overall_syntax
      }
    Map("kinds" -> syntax.keywords.kinds)
  }

  /* hover information and definitions (as Isabelle/jEdit's tooltips and hyperlinks) */

  def info(file: JFile, offset: Int): Obj = {
    val st = state.value
    st.models.get(file) match {
      case None => Map("tooltip" -> null, "definition" -> null)
      case Some(model) =>
        val snapshot = session.snapshot(model.node_name, st.pending_edits)
        val o16 = model.to16(offset)
        val range = Text.Range(o16, o16 + 1)
        val rendering = new Rendering(snapshot, options, session)
        val tooltip =
          rendering.tooltips(Rendering.tooltip_elements, range).map {
            case Text.Info(r, elems) =>
              Map(
                "start" -> model.from16(r.start), "end" -> model.from16(r.stop),
                "items" -> elems.map { e =>
                  val (raw, spans) = styled(List(e), 80.0)
                  Map("kind" -> message_kind(e.name), "raw" -> raw, "text" -> Symbol.decode(raw), "spans" -> spans)
                })
          }
        def place(pos: Position.T): Option[Obj] =
          pos match {
            case Position.Item_Def_File(name, line, _) =>
              session.resources.source_file(name).map(path =>
                Map("file" -> File.absolute(new JFile(path)).getPath, "line" -> line))
            case Position.Item_Def_Id(id, r) =>
              snapshot.find_command_position(id, r.start).map(p =>
                Map("file" -> p.name, "line" -> (p.pos.line + 1), "column" -> p.pos.column))
            case _ => None
          }
        val definition =
          snapshot.cumulate[Option[Obj]](range, None, Markup.Elements(Markup.ENTITY), _ =>
            {
              case (None, Text.Info(r, XML.Elem(Markup(Markup.ENTITY, props), _))) =>
                place(props).map(p => Some(p ++ Map(
                  "name" -> Markup.Name.unapply(props).getOrElse(""),
                  "kind" -> Markup.Kind.unapply(props).getOrElse(""),
                  "start" -> model.from16(r.start), "end" -> model.from16(r.stop))))
              case _ => None
            }).collectFirst { case Text.Info(_, Some(d)) => d }
        Map("tooltip" -> tooltip.orNull, "definition" -> definition.orNull)
    }
  }

  private def handle(obj: Obj): Unit = {
    val id = obj.getOrElse("id", null)
    try {
      JSON.string(obj, "op") getOrElse "" match {
        case "open" =>
          open(file_of(obj), JSON.string(obj, "text") getOrElse "", JSON.long(obj, "rev"))
          if (id != null) reply(id, "ok" -> true)
        case "edit" =>
          val edits =
            JSON.list(obj, "edits", e =>
              for {
                start <- JSON.int(e, "start")
                stop <- JSON.int(e, "end")
                text <- JSON.string(e, "text")
              } yield (start, stop, text)) getOrElse Nil
          edit(file_of(obj), edits, JSON.long(obj, "rev"))
          if (id != null) reply(id, "ok" -> true)
        case "close" =>
          close(file_of(obj))
          if (id != null) reply(id, "ok" -> true)
        case "disk_changed" =>
          disk_changed(JSON.strings(obj, "files").getOrElse(Nil).map(new JFile(_)))
        case "checking" =>
          set_checking(JSON.bool(obj, "on") getOrElse true)
          if (id != null) reply(id, "ok" -> true)
        case "cancel" =>
          val n = cancel()
          if (id != null) reply(id, "canceled" -> n)
        case "cancel_tools" =>
          val n = cancel_tools()
          if (id != null) reply(id, "canceled" -> n)
        case "state" =>
          state_query(id, file_of(obj), JSON.int(obj, "offset") getOrElse 0,
            JSON.double(obj, "margin") getOrElse 100.0)
        case "query" =>
          start_query(JSON.string(obj, "query") getOrElse error("Missing query id"),
            file_of(obj), JSON.int(obj, "offset") getOrElse 0,
            JSON.string(obj, "operation") getOrElse error("Missing operation"),
            JSON.strings(obj, "args") getOrElse Nil)
          if (id != null) reply(id, "ok" -> true)
        case "cancel_query" =>
          cancel_query(JSON.string(obj, "query") getOrElse "")
          if (id != null) reply(id, "ok" -> true)
        case "syntax" =>
          reply(id, "syntax" -> syntax_json(JSON.string(obj, "file").map(new JFile(_))))
        case "symbols" =>
          reply(id, "symbols" -> symbols_json)
        case "info" =>
          reply(id, "info" -> info(file_of(obj), JSON.int(obj, "offset") getOrElse 0))
        case "options" =>
          // System options for the running prover, e.g. sledgehammer_timeout=60
          val updated = JSON.strings(obj, "set").getOrElse(Nil).foldLeft(options)(_ + _)
          options = updated
          session.update_options(updated)
          if (id != null) reply(id, "ok" -> true)
        case "ping" =>
          reply(id, "ok" -> true)
        case op => if (id != null) reply(id, "error" -> ("Unknown op: " + op))
      }
    }
    catch {
      case exn: Throwable if !Exn.is_interrupt(exn) =>
        if (id != null) reply(id, "error" -> Exn.message(exn))
        else log("Error: " + Exn.message(exn))
    }
  }


  /* main loop */

  def run(): Unit = {
    try {
      val session_background =
        Sessions.background(options, logic, dirs = dirs, include_sessions = include_sessions)
          .check_errors

      def build(no_build: Boolean): Build.Results = {
        val progress = new Progress {
          override def verbose: Boolean = false
          override def output(message: Progress.Message): Unit =
            if (do_output(message)) event("build", "text" -> message.text)
        }
        Build.build(options, selection = Sessions.Selection.session(session_background.session_name),
          progress = progress, build_heap = true, no_build = no_build, dirs = dirs,
          infos = session_background.infos)
      }

      if (!no_build && !build(no_build = true).ok) {
        event("phase", "phase" -> "building")
        if (!build(no_build = false).ok) error("Session build failed for " + logic)
      }

      val resources = new Backend_Resources(options, session_background, backend)
      val session = new Session(options, resources)
      _session = Some(session)

      session.commands_changed += prover_output
      session.phase_changed += phase_output
      session.syslog_messages += syslog_output

      val store = Store(options)
      val session_heaps =
        ML_Process.session_heaps(store, session_background, logic = session_background.session_name)
      val prelude = Path.explode("$DEDUCTUM_BACKEND_HOME/ML/deductum_prelude.ML")
      val use_prelude = if (prelude.is_file) List(File.platform_path(prelude.expand)) else Nil
      Isabelle_Process.start(options, session, session_background, session_heaps,
        use_prelude = use_prelude).await_startup()
      event("ready", "logic" -> session_background.session_name,
        "identifier" -> Isabelle_System.getenv("ISABELLE_IDENTIFIER"),
        "home" -> Isabelle_System.getenv("ISABELLE_HOME"))
    }
    catch {
      case exn: Throwable =>
        event("phase", "phase" -> "failed", "message" -> Exn.message(exn))
        sys.exit(2)
    }

    val in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8))
    var line = in.readLine()
    while (line != null) {
      if (line.trim.nonEmpty) {
        JSON.parse(line) match {
          case obj: Map[_, _] => handle(obj.asInstanceOf[Obj])
          case _ => log("Bad request: " + line)
        }
      }
      line = in.readLine()
    }

    stopping = true
    delay_input.revoke(); delay_output.revoke(); delay_load.revoke()
    session.stop()
    sys.exit(0)
  }
}
