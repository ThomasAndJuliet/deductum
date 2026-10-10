#!/usr/bin/env python3
"""Exercise remote forwarding preparation with real Unix sockets."""
import importlib.util
import os
from pathlib import Path
import socket
import tempfile
import unittest
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1] / "src/agent/sudo-forward.py"
spec = importlib.util.spec_from_file_location("sudo_forward", SOURCE)
forward = importlib.util.module_from_spec(spec)
spec.loader.exec_module(forward)


class ForwardPreparation(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = str(Path(self.tmp.name) / "sudo.sock")

    def listener(self):
        listener = socket.socket(socket.AF_UNIX)
        self.addCleanup(listener.close)
        listener.bind(self.path)
        listener.listen()
        return listener

    def test_local_agent_is_detached_so_ssh_can_bind(self):
        old = self.listener()
        with patch.object(forward, "local_agent", return_value=True):
            self.assertEqual(forward.prepare(self.path), self.path)
        new = self.listener()
        with socket.socket(socket.AF_UNIX) as client:
            client.connect(self.path)
            accepted, _ = new.accept()
            accepted.close()
        old.settimeout(0.01)
        # The old listener still exists but cannot receive pathname connects.
        old.accept()[0].close()  # prepare's health check
        with self.assertRaises(TimeoutError):
            old.accept()

    def test_existing_ssh_forward_is_preserved(self):
        self.listener()
        inode = os.lstat(self.path).st_ino
        with patch.object(forward, "local_agent", return_value=False):
            self.assertEqual(forward.prepare(self.path), "live")
        self.assertEqual(os.lstat(self.path).st_ino, inode)

    def test_dead_socket_and_missing_path_can_be_bound(self):
        self.assertEqual(forward.prepare(self.path), self.path)
        self.listener().close()
        self.assertEqual(forward.prepare(self.path), self.path)
        self.assertFalse(os.path.lexists(self.path))

    def test_files_and_symlinks_are_never_removed(self):
        Path(self.path).write_text("keep me")
        with self.assertRaises(RuntimeError):
            forward.prepare(self.path)
        self.assertEqual(Path(self.path).read_text(), "keep me")
        os.unlink(self.path)
        os.symlink("missing", self.path)
        with self.assertRaises(RuntimeError):
            forward.prepare(self.path)
        self.assertTrue(os.path.islink(self.path))

    def test_peer_process_distinguishes_local_agent_from_sshd(self):
        peer = (123, os.getuid(), os.getgid())
        for exe, expected in [("/usr/lib/systemd/systemd", True), ("/home/user/ct-sudo-agent-abcd", True), ("/usr/libexec/sshd-session", False)]:
            with patch.object(os, "readlink", return_value=exe):
                self.assertEqual(forward.local_agent(peer), expected)
        with patch.object(os, "readlink", side_effect=PermissionError), patch.object(Path, "read_text", side_effect=PermissionError):
            self.assertFalse(forward.local_agent(peer))

    def test_hidden_executable_uses_readable_process_name(self):
        peer = (123, os.getuid(), os.getgid())
        for name, expected in [("systemd\n", True), ("ct-sudo-agent-f\n", True), ("sshd-session\n", False)]:
            with patch.object(os, "readlink", side_effect=PermissionError), patch.object(Path, "read_text", return_value=name):
                self.assertEqual(forward.local_agent(peer), expected)


if __name__ == "__main__":
    unittest.main()
