"""Disk bytes and native path boundaries, including real Windows sharing denial."""
import asyncio
import os
from pathlib import Path
import sys
import tempfile
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
        with tempfile.TemporaryDirectory(prefix="Deductum Å proof ") as temp:
            path = Path(temp) / "Å proof 😀 ' ; $literal.thy"
            path.write_text("theory Main imports Main begin end", encoding="utf-8")
            cygwin = adapter.to_posix(str(path))
            self.assertTrue(cygwin.startswith("/"), "Isabelle shell boundary did not produce an absolute POSIX path")
            self.assertTrue(os.path.samefile(adapter.to_native(cygwin), path))
            obj = adapter.protocol({"file": cygwin, "files": [cygwin], "text": "/literal/😀", "start": 9, "end": 13})
            self.assertTrue(os.path.samefile(obj["file"], path))
            self.assertTrue(os.path.samefile(obj["files"][0], path))
            self.assertEqual((obj["text"], obj["start"], obj["end"]), ("/literal/😀", 9, 13))


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
