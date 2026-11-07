"""Disk bytes and native path boundaries, including real Windows sharing denial."""
import asyncio
import base64
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import deductum_host as H
from drivers.lean import path_from_uri
from drivers import lean
from drivers.isabelle import RuntimeAdapter
import platform_support as P


class FileBytes(unittest.IsolatedAsyncioTestCase):
    async def test_read_edit_save_retains_the_original_utf8_bom_and_line_endings(self):
        cases = [(b"", "", False), (b"abc", "abc", False), (b"abc\n", "abc\n", False),
                 ("Å∀é😀\nx\n".encode("utf-8"), "Å∀é😀\nx\n", False),
                 ("Å∀é😀\r\nx\r\n".encode("utf-8"), "Å∀é😀\nx\n", True)]
        with tempfile.TemporaryDirectory(prefix="Deductum Å proof ") as temp:
            host = H.Host()
            with patch.object(H, "STATE_DIR", Path(temp) / "state"):
                session = H.Session(host, "files", {"folder": temp, "backend": "lean", "lifecycle": "Stopped"})
                for bom in (False, True):
                    for index, (raw, expected, crlf) in enumerate(cases):
                        with self.subTest(bom=bom, case=index):
                            path = Path(temp) / f"Å proof 😀 {bom}-{index}.lean"
                            path.write_bytes((b"\xef\xbb\xbf" if bom else b"") + raw)
                            doc = session.get_doc(str(path))
                            self.assertEqual((doc.text, doc.crlf, doc.bom), (expected, crlf, bom))
                            session.commit(doc, H.HUMAN, "insert", [(0, 0, "∀😀\n")])
                            self.assertTrue(session.save_doc(doc), doc.save_error)
                            prefix = "∀😀\r\n" if crlf else "∀😀\n"
                            expected_bytes = (b"\xef\xbb\xbf" if bom else b"") + prefix.encode("utf-8") + raw
                            self.assertEqual(path.read_bytes(), expected_bytes, "save changed encoding or line endings")
                            self.assertFalse(doc.dirty)
                            self.assertEqual(H.read_file(str(path)), ("∀😀\n" + expected, crlf, bom))
                for doc in session.docs.values():
                    if doc.save_handle:
                        doc.save_handle.cancel()

    def test_file_uri_round_trip_keeps_spaces_unicode_and_non_bmp_characters(self):
        with tempfile.TemporaryDirectory(prefix="Deductum Å proof ") as temp:
            path = Path(temp) / "Å 😀 ' ; $literal.lean"
            path.touch()
            uri = path.as_uri()
            if os.name == "nt":
                self.assertRegex(uri, r"^file:///[A-Za-z]:/")
            self.assertTrue(os.path.samefile(path_from_uri(uri), path))

    @unittest.skipUnless(os.name == "nt", "Native NTFS replacement sharing semantics")
    async def test_sharing_violation_preserves_disk_bytes_and_dirty_text_until_retry(self):
        import ctypes as C
        from ctypes import wintypes as W
        kernel = C.WinDLL("kernel32", use_last_error=True)
        create = kernel.CreateFileW
        create.argtypes = [W.LPCWSTR, W.DWORD, W.DWORD, W.LPVOID, W.DWORD, W.DWORD, W.HANDLE]
        create.restype = W.HANDLE
        kernel.CloseHandle.argtypes = [W.HANDLE]
        with tempfile.TemporaryDirectory() as temp, patch.object(H, "STATE_DIR", Path(temp) / "state"):
            path = Path(temp) / "locked.lean"
            path.write_bytes(b"original\r\n")
            host = H.Host()
            session = H.Session(host, "locked", {"folder": temp, "backend": "lean", "lifecycle": "Stopped"})
            doc = session.get_doc(str(path))
            session.commit(doc, H.HUMAN, "insert", [(0, 0, "new\n")])
            handle = create(str(path), 0x80000000, 1 | 2, None, 3, 0x80, None)
            self.assertNotEqual(handle, C.c_void_p(-1).value, "fixture could not open a non-delete-sharing handle")
            try:
                self.assertFalse(session.save_doc(doc), "save succeeded despite sharing denial")
                self.assertTrue(doc.save_error)
                self.assertTrue(doc.dirty)
                self.assertEqual(doc.text, "new\noriginal\n")
                self.assertEqual(path.read_bytes(), b"original\r\n")
            finally:
                kernel.CloseHandle(handle)
            self.assertTrue(session.save_doc(doc), doc.save_error)
            self.assertFalse(doc.dirty)
            self.assertEqual(path.read_bytes(), b"new\r\noriginal\r\n")
            if doc.save_handle:
                doc.save_handle.cancel()


class IsabellePathBoundary(unittest.TestCase):
    def test_windows_cygwin_options_disable_native_argv_globbing_without_changing_the_callers_environment(self):
        adapter = RuntimeAdapter("unused")
        for path_key in ("PATH", "Path", "pAtH", None):
            for options in (None, "winsymlinks:nativestrict", "glob winsymlinks:nativestrict"):
                original = {"LANG": "old-locale"}
                if path_key:
                    original[path_key] = "unrelated tools"
                if options is not None:
                    original["CYGWIN"] = options
                before = dict(original)
                with self.subTest(options=options, path_key=path_key), patch.object(P, "WINDOWS", True):
                    result = adapter.environment(original)
                self.assertEqual(original, before, "runtime preparation changed the parent environment")
                path_keys = [key for key in result if key.upper() == "PATH"]
                self.assertEqual(path_keys, ["PATH"], "child environment contains conflicting Windows Path keys")
                self.assertEqual(result["PATH"], str(adapter.bash.parent) + ";" + ("unrelated tools" if path_key else ""))
                self.assertEqual((result["LANG"], result["LC_ALL"]), ("C.UTF-8", "C.UTF-8"))
                tokens = result["CYGWIN"].split()
                self.assertEqual(tokens[-1], "noglob", "native argv is still interpreted by the Cygwin startup globber")
                for option in (options or "").split():
                    self.assertIn(option, tokens, "an unrelated Cygwin option was discarded")

    def test_encoded_argument_limit_is_bounded_before_any_runtime_execution(self):
        adapter = RuntimeAdapter("unused")
        # The specified vector is "2 NUL /r NUL argument NUL": six bytes
        # besides the argument. Exactly 12288 decoded bytes encode to 16384.
        with patch.object(P, "WINDOWS", True), patch.object(Path, "is_file", return_value=True), \
                patch.object(adapter, "cli_path", return_value="/r"), \
                patch.object(P, "run", side_effect=AssertionError("runtime started")):
            command = adapter.command(["x" * 12282])
            self.assertEqual(len(command[-1]), 16384)
            for invalid in ("x" * 12283, "before\0after"):
                with self.subTest(length=len(invalid)), self.assertRaises(H.HostError) as caught:
                    adapter.run([invalid])
                self.assertEqual(caught.exception.code, "unsupported_path")
                self.assertIn("16384" if "\0" not in invalid else "NUL", str(caught.exception))

    def test_unix_cli_paths_and_arguments_are_preserved(self):
        adapter = RuntimeAdapter("/opt/Isabelle β/bin/isabelle")
        with patch.object(P, "WINDOWS", False):
            self.assertEqual(adapter.cli_path("/tmp/Ω project"), "/tmp/Ω project")
            self.assertEqual(adapter.command(["build", "-d", "/tmp/Ω project", "Logicβ"]),
                             ["/opt/Isabelle β/bin/isabelle", "build", "-d", "/tmp/Ω project", "Logicβ"])

    def test_invalid_command_output_reports_encoding_and_exit_status_instead_of_none_stdout(self):
        adapter = RuntimeAdapter("unused", cancel_event=threading.Event())
        for stream, capture in ((2, {"capture_output": True}),
                                (1, {"stdout": subprocess.PIPE, "stderr": subprocess.DEVNULL})):
            code = f"import os; os.write({stream},bytes([255])); raise SystemExit(7)"
            with self.subTest(stream=stream), patch.object(adapter, "command", return_value=[sys.executable, "-c", code]):
                with self.assertRaises(H.HostError) as caught:
                    adapter.run([], text=True, encoding="utf-8", timeout=10, **capture)
            self.assertEqual(caught.exception.code, "bad_encoding")
            self.assertIn("stderr" if stream == 2 else "stdout", str(caught.exception))
            self.assertIn("exit 7", str(caught.exception))
            if stream == 2:
                self.assertIn("\\xff", str(caught.exception), "diagnostic lost the invalid raw output")

    def test_protocol_converts_only_structured_paths_preserving_proof_text_and_offsets(self):
        adapter = RuntimeAdapter("unused")
        native = "C:\\Å proof 😀\\Main.thy"
        original = {"file": "/cygdrive/c/Å proof 😀/Main.thy", "text": "/literal/text 😀", "start": 7,
                    "end": 12, "nested": [{"home": "/cygdrive/c/Å proof 😀/Main.thy"}],
                    "files": ["/cygdrive/c/Å proof 😀/Main.thy"], "imports": ["Main"]}
        with patch.object(P, "WINDOWS", True), patch.object(adapter, "to_native", return_value=native):
            result = adapter.protocol(original)
        self.assertEqual(result["file"], native)
        self.assertEqual(result["nested"], [{"home": native}])
        self.assertEqual(result["files"], [native])
        self.assertEqual(result["imports"], ["Main"])
        for key in ("text", "start", "end"):
            self.assertEqual(result[key], original[key], f"path adapter changed {key}")
        self.assertEqual(original["file"], "/cygdrive/c/Å proof 😀/Main.thy", "adapter mutated the caller's object")

    @unittest.skipUnless(os.name == "nt", "Official Windows Isabelle bundled-Cygwin boundary")
    def test_native_unicode_paths_round_trip_through_the_official_bundled_cygpath(self):
        runtime = os.environ.get("DEDUCTUM_ISABELLE")
        if not runtime:
            self.skipTest("Native Isabelle path acceptance requires DEDUCTUM_ISABELLE pointing to official runtime")
        adapter = RuntimeAdapter(runtime)
        with tempfile.TemporaryDirectory(prefix="Deductum β Ω proof ") as temp:
            path = Path(temp) / "β Ω 😀 ' ; $literal.thy"
            path.write_text("theory Main imports Main begin end", encoding="utf-8")
            cygwin = adapter.to_posix(str(path))
            self.assertTrue(cygwin.startswith("/"), "Isabelle shell boundary did not produce an absolute POSIX path")
            self.assertTrue(os.path.samefile(adapter.to_native(cygwin), path))
            obj = adapter.protocol({"file": cygwin, "files": [cygwin], "text": "/literal/😀", "start": 9, "end": 13})
            self.assertTrue(os.path.samefile(obj["file"], path))
            self.assertTrue(os.path.samefile(obj["files"][0], path))
            self.assertEqual((obj["text"], obj["start"], obj["end"]), ("/literal/😀", 9, 13))
            alias = adapter.cli_path(str(path))
            self.assertTrue(alias.isascii(), "native Java CLI still receives a non-ASCII path")
            self.assertTrue(os.path.samefile(adapter.to_native(alias), path))
            self.assertEqual(obj["file"], str(path.resolve()), "a CLI alias leaked into document identity")

    @unittest.skipUnless(os.name == "nt", "Official Windows Isabelle CLI compatibility boundary")
    def test_non_ascii_logic_names_are_refused_before_starting_the_runtime(self):
        runtime = os.environ.get("DEDUCTUM_ISABELLE")
        if not runtime:
            self.skipTest("Requires official DEDUCTUM_ISABELLE runtime")
        adapter = RuntimeAdapter(runtime)
        for args in (["build", "Logicβ"], ["process", "-o", "option=Ω"]):
            with self.subTest(args=args), patch.object(P, "run", side_effect=AssertionError("runtime started")):
                with self.assertRaises(H.HostError) as caught:
                    adapter.run(args)
                self.assertEqual(caught.exception.code, "unsupported_path")
                self.assertIn("ASCII", str(caught.exception))


@unittest.skipUnless(os.name == "nt", "Official Windows bundled Bash argument transport")
class WindowsIsabelleArguments(unittest.TestCase):
    def setUp(self):
        runtime = os.environ.get("DEDUCTUM_ISABELLE")
        if not runtime:
            self.skipTest("Requires the official DEDUCTUM_ISABELLE runtime")
        self.temp = tempfile.TemporaryDirectory(prefix="Deductum β Ω transport ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.output = self.root / "argv.bin"
        self.output.touch()
        receiver = self.root / "receive args.sh"
        receiver.write_bytes(b"output=$1\nshift\nprintf '%s\\0' \"$@\" > \"$output\"\n"
                             b"/usr/bin/cat > \"$output.stdin\"\n"
                             b"printf 'receiver stdout\\n'\nprintf 'receiver stderr\\n' >&2\nexit 17\n")
        self.adapter = RuntimeAdapter(runtime)
        # Keep the actual official Bash/Cygpath, selecting a private receiver
        # through the same public runtime field used by command().
        self.adapter.runtime = str(receiver)
        self.receiver = self.adapter.cli_path(str(receiver))
        self.destination = self.adapter.cli_path(str(self.output))

    def test_official_bash_receives_exact_arguments_stdin_and_exit_status(self):
        arguments = ["", "space value", 'literal "double quotes"', "apostrophe's", "$HOME $(touch SHOULD_NOT_EXIST)",
                     "`touch SHOULD_NOT_EXIST`", "semi;colon & pipe|", "slash\\", "double\\\\", 'mixed\\\"end\\',
                     "first\nsecond\r\nthird", "__DEDUCTUM_ARGV_END__", "x" * 9000, ""]
        standard_input = b"raw\0\xff\r\n\x1a" + bytes(range(256))
        result = self.adapter.run([self.destination, *arguments], input=standard_input,
                                  cwd=str(self.root), capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 17)
        self.assertEqual(result.stdout, b"receiver stdout\n")
        self.assertEqual(result.stderr, b"receiver stderr\n")
        self.assertEqual(self.output.read_bytes(), b"\0".join(arg.encode("ascii") for arg in arguments) + b"\0",
                         "the official Bash boundary changed argument data or empty arguments")
        self.assertEqual(Path(str(self.output) + ".stdin").read_bytes(), standard_input)
        self.assertFalse((self.root / "SHOULD_NOT_EXIST").exists(), "argument contents were evaluated")

    def test_async_managed_launch_preserves_the_same_argument_vector_and_streams(self):
        arguments = ["", 'quoted "value"', "last\\", "'single' $literal\nnext", ""]
        async def run():
            proc = await P.create_subprocess_exec(*self.adapter.command([self.destination, *arguments]),
                env=self.adapter.environment(), cwd=str(self.root), stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
            out, err = await asyncio.wait_for(proc.communicate(b"async\0\xff"), 30)
            return proc.returncode, out, err
        code, out, err = asyncio.run(run())
        self.assertEqual((code, out, err), (17, b"receiver stdout\n", b"receiver stderr\n"))
        self.assertEqual(self.output.read_bytes(), b"\0".join(arg.encode("ascii") for arg in arguments) + b"\0")
        self.assertEqual(Path(str(self.output) + ".stdin").read_bytes(), b"async\0\xff")

    def test_malformed_payloads_never_execute_even_a_partially_decoded_receiver(self):
        launcher = self.adapter.cli_path(str(Path(__file__).resolve().parent.parent / "drivers" / "isabelle_windows.sh"))
        vector = [self.receiver, self.destination, ""]
        def wire(count, values, final_nul=True):
            raw = (str(count) + "\0" + "\0".join(values)).encode("ascii")
            return raw + (b"\0" if final_nul else b"")
        raw = wire(3, vector)
        vector[-1] = "x" * ((-len(raw)) % 3)
        valid = base64.b64encode(wire(3, vector)).decode("ascii")
        self.assertNotIn("=", valid, "partial-decode case must reach decoder exit-status validation")
        invalid = {"empty": [""], "alphabet": ["!!!!"], "truncated_base64": [valid + "A"],
                   "no_final_nul": [base64.b64encode(wire(3, vector, False)).decode("ascii")],
                   "too_few_fields": [base64.b64encode(wire(4, vector)).decode("ascii")],
                   "too_many_fields": [base64.b64encode(wire(2, vector)).decode("ascii")],
                   "zero_count": [base64.b64encode(wire(0, vector)).decode("ascii")],
                   "count_expression": [base64.b64encode(wire("1+2", vector)).decode("ascii")],
                   "oversize": ["A" * 16385], "missing_payload": [], "extra_payload": [valid, valid]}
        for case, payload in invalid.items():
            with self.subTest(case=case):
                self.output.write_bytes(b"not executed")
                result = P.run([str(self.adapter.bash), "--noprofile", "--norc", launcher, *payload],
                               env=self.adapter.environment(), input=b"", capture_output=True, timeout=30)
                self.assertEqual(result.returncode, 2, result.stderr.decode(errors="replace"))
                self.assertIn(b"Invalid Deductum Isabelle argument payload", result.stderr)
                self.assertEqual(self.output.read_bytes(), b"not executed", "decoder executed a partial vector")
                self.assertFalse(Path(str(self.output) + ".stdin").exists())


@unittest.skipUnless(os.name == "nt", "Native existing NTFS short-name identity")
class WindowsCliAliases(unittest.TestCase):
    def test_existing_ascii_alias_resolves_to_the_same_unicode_file_without_creating_a_new_name(self):
        import _windows as W
        with tempfile.TemporaryDirectory(prefix="Deductum β Ω ") as temp:
            path = Path(temp) / "proof β Ω.thy"
            path.write_text("theory Demo imports Main begin end", encoding="utf-8")
            before = set(Path(temp).iterdir())
            alias = W.ascii_short_path(str(path))
            self.assertTrue(alias.isascii())
            self.assertTrue(os.path.samefile(alias, path))
            self.assertEqual(os.path.normcase(os.path.realpath(alias)), os.path.normcase(str(path.resolve())))
            self.assertEqual(set(Path(temp).iterdir()), before, "alias lookup created a filesystem entry")

    def test_missing_non_ascii_and_truncated_short_names_give_actionable_ascii_path_guidance(self):
        import _windows as W
        with tempfile.TemporaryDirectory(prefix="Deductum β ") as temp:
            path = Path(temp) / "Ω.thy"
            path.touch()
            for label, name, size in (("missing", "", 0), ("unicode", str(path), len(str(path))),
                                       ("oversized", "C:\\SHORT.THY", 32768)):
                def get_short(original, output, capacity):
                    self.assertEqual(original, str(path))
                    output.value = name
                    return size
                with self.subTest(case=label), patch.object(W, "api", return_value=get_short):
                    with self.assertRaises(OSError) as caught:
                        W.ascii_short_path(str(path))
                    self.assertIn("ASCII", str(caught.exception))
                    self.assertIn("move", str(caught.exception))

    def test_alias_to_a_different_existing_file_is_rejected(self):
        import _windows as W
        with tempfile.TemporaryDirectory(prefix="Deductum β ") as temp:
            path = Path(temp) / "Ω.thy"
            path.write_text("original", encoding="utf-8")
            other = Path(temp) / "other.thy"
            other.write_text("unrelated", encoding="utf-8")
            wrong_alias = W.ascii_short_path(str(other))
            def get_short(original, output, capacity):
                output.value = wrong_alias
                return len(wrong_alias)
            with patch.object(W, "api", return_value=get_short):
                with self.assertRaises(OSError) as caught:
                    W.ascii_short_path(str(path))
            self.assertIn("does not identify", str(caught.exception))
            self.assertEqual(path.read_text(encoding="utf-8"), "original")
            self.assertEqual(other.read_text(encoding="utf-8"), "unrelated")

    def test_adapter_reports_an_unusable_alias_as_a_path_compatibility_error(self):
        import _windows as W
        adapter = RuntimeAdapter("unused")
        with patch.object(W, "ascii_short_path", side_effect=OSError("move to an ASCII path")):
            with self.assertRaises(H.HostError) as caught:
                adapter.cli_path("C:\\project β\\Demo.thy")
        self.assertEqual(caught.exception.code, "unsupported_path")
        self.assertIn("ASCII", str(caught.exception))


@unittest.skipUnless(os.name == "nt", "Native Windows drive-letter identity in Lean definition URIs")
class LeanDefinitionIdentity(unittest.TestCase):
    def test_lowercase_drive_uri_uses_the_authoritative_buffer_instead_of_stale_disk_text(self):
        with tempfile.TemporaryDirectory(prefix="Deductum Å proof ") as temp:
            path = Path(temp) / "Main.lean"
            path.write_text("theorem old : True := by trivial\n", encoding="utf-8")
            class Session:
                meta = {"folder": temp}
            driver = lean.LeanDriver(Session())
            text = "-- 😀 unsaved buffer\n\ntheorem fresh : True := by trivial\n"
            doc = lean.LDoc(str(path), text, 1)
            driver.docs[str(path)] = doc
            uri = path.as_uri().replace("file:///" + path.drive, "file:///" + path.drive.lower(), 1)
            definition = driver._definition([{"uri": uri, "range": {
                "start": {"line": 2, "character": 8}, "end": {"line": 2, "character": 13}}}])
            self.assertIsNotNone(definition)
            self.assertEqual(definition["name"], "fresh")
            self.assertEqual(definition["kind"], "theorem")
            self.assertEqual(text[definition["start"]:definition["end"]], "fresh")
            self.assertEqual(definition["line"], 3)
            self.assertEqual(definition["file"], doc.path)


if __name__ == "__main__":
    unittest.main()
