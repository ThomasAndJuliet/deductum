"""Component preparation through its real files and external Isabelle boundary.

The fake runtime builds only after observing a complete registered component.
The native integration still exercises the official JVM and real proof backend.
"""
import concurrent.futures
import contextlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from drivers import HostError
from drivers import isabelle
import platform_support as platform


INPUTS = {
    "etc/settings": b'# Fixture version 1\nDEDUCTUM_BACKEND_HOME="$COMPONENT"\n',
    "etc/build.props": b"title = Test backend\nmodule = lib/deductum_backend.jar\n"
                       b"sources = src/backend.scala\nservices = deductum.Tools\n",
    "src/backend.scala": b"object Fixture { val value = 1 }\n",
    "ML/deductum_prelude.ML": b"val fixture = 1;\n",
}


def write_component(path, contents=INPUTS):
    for name, data in contents.items():
        destination = path / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)


def file_bytes(path):
    return {str(p.relative_to(path)).replace(os.sep, "/"): p.read_bytes()
            for p in path.rglob("*") if p.is_file()}


class ComponentRuntime:
    """Only the official process is replaced; registry/cache files stay real."""
    def __init__(self, home_user):
        self.home_user = home_user
        self.registry = home_user / "etc" / "components"
        self.registry.parent.mkdir(parents=True)
        self.registry.write_text("", encoding="utf-8")
        self.builds = []
        self.before_build = None
        self.fail_next_build = False
        self.mutex = threading.Lock()
        self.native_paths = {}
        self.cli_paths = {}

    def to_native(self, path):
        return str(self.native_paths.get(str(path), path))

    def to_posix(self, path):
        return str(path)

    def cli_path(self, path):
        return str(self.cli_paths.get(str(path), path))

    def native_entry(self, entry):
        path = Path(self.to_native(entry))
        return path if path.is_absolute() else self.home_user / path

    def entries(self):
        return [line.strip() for line in self.registry.read_text(encoding="utf-8").splitlines()
                if line.strip() and not line.lstrip().startswith("#")]

    def component(self):
        paths = [self.native_entry(entry) for entry in self.entries()]
        matches = [path for path in paths if (path / "src" / "backend.scala").is_file()
                   and (path / "ML" / "deductum_prelude.ML").is_file()
                   and (path / "etc" / "settings").is_file()
                   and 'DEDUCTUM_BACKEND_HOME="$COMPONENT"' in
                   (path / "etc" / "settings").read_text(encoding="utf-8")]
        if len(matches) != 1:
            raise AssertionError(f"Expected one registered backend, found {matches}")
        return matches[0]

    def run(self, args, **kwargs):
        if args == ["getenv", "-b", "ISABELLE_HOME_USER"]:
            return subprocess.CompletedProcess(args, 0, str(self.home_user) + "\n", "")
        if args[:1] == ["components"]:
            # Unix uses the official components command. Windows must repair
            # the registry before a broken old classpath can start the JVM.
            lines = self.registry.read_text(encoding="utf-8").splitlines()
            if platform.WINDOWS and any(
                    any(ord(c) > 0xffff for c in str(self.native_entry(entry))) and
                    (self.native_entry(entry) / "lib" / "deductum_backend.jar").is_file()
                    for entry in self.entries()):
                return subprocess.CompletedProcess(args, 1, "", "Error decoding percent encoded characters")
            if args[1] == "-x":
                lines = [line for line in lines if line.strip() != args[2]]
            elif args[1] == "-u":
                lines.append(args[2])
            else:
                raise AssertionError(f"Unexpected components operation: {args}")
            self.registry.write_text("\n".join(lines) + "\n", encoding="utf-8")
            return subprocess.CompletedProcess(args, 0, "", "")
        if args == ["scala_build"]:
            component = self.component()
            jar = component / "lib" / "deductum_backend.jar"
            # Official scala_build may trust an existing jar's embedded source
            # checksum even when its class bytes are damaged. Reinvocation is
            # therefore not an oracle that corrupt output was repaired.
            incremental_noop = jar.is_file()
            actual = file_bytes(component)
            for name in INPUTS:
                if name not in actual:
                    raise AssertionError(f"Build saw incomplete component: missing {name}")
            if self.before_build:
                self.before_build(component)
            with self.mutex:
                self.builds.append(component)
                if self.fail_next_build:
                    self.fail_next_build = False
                    return subprocess.CompletedProcess(args, 19, "", "fixture compilation failed")
                if incremental_noop:
                    return subprocess.CompletedProcess(args, 0, "Sources unchanged\n", "")
                jar.parent.mkdir(exist_ok=True)
                jar.write_bytes(b"compiled fixture jar\n")
            return subprocess.CompletedProcess(args, 0, "Built backend\n", "")
        raise AssertionError(f"Unexpected Isabelle invocation: {args}")


class IsabelleComponents(unittest.TestCase):
    def setUp(self):
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        self.root = Path(self.stack.enter_context(tempfile.TemporaryDirectory(prefix="deductum-components-")))
        self.share = self.root / "Deductum β proof 😀" / "share" / "deductum"
        self.source = self.share / "backend" / "Isabelle2025"
        write_component(self.source)
        self.state = self.root / "private-state"
        runtime_home = self.root / "official-runtime"
        (runtime_home / "etc").mkdir(parents=True)
        (runtime_home / "etc" / "ISABELLE_IDENTIFIER").write_text("Isabelle2025\n", encoding="utf-8")
        (runtime_home / "bin").mkdir()
        self.runtime = str(runtime_home / "bin" / "isabelle")
        self.process = ComponentRuntime(self.root / "isabelle-user")
        self.processes = {self.runtime: self.process}
        self.stack.enter_context(patch.object(isabelle, "SHARE_DIR", self.share))
        self.stack.enter_context(patch.object(isabelle, "_component_ready", {}))
        self.stack.enter_context(patch.object(isabelle, "RuntimeAdapter",
                                             side_effect=lambda runtime, **k: self.processes[runtime]))
        self.stack.enter_context(patch.object(platform, "state_dir", return_value=self.state))
        if os.name != "nt":
            # Exercise the actual copy/identity/registry algorithms on Linux;
            # native CI separately enforces DACLs and cross-process locking.
            self.stack.enter_context(patch.object(isabelle, "_component_secure_directory",
                                                 side_effect=lambda path: Path(path).mkdir(parents=True, exist_ok=True)))
            self.stack.enter_context(patch.object(isabelle, "_component_secure_tree",
                                                 side_effect=lambda *a, **k: None))
            self.stack.enter_context(patch.object(isabelle, "_component_file_lock",
                                                 side_effect=lambda *a, **k: contextlib.nullcontext()))
            self.stack.enter_context(patch.object(isabelle, "_component_replace_registry",
                                                 side_effect=os.replace))
        self.progress = []

    def prepare(self, cancel_event=None):
        isabelle.ensure_component(self.runtime, self.progress.append, cancel_event=cancel_event)
        return self.process.component()

    def test_windows_build_uses_a_complete_private_component_without_changing_the_package(self):
        original = file_bytes(self.source)
        with patch.object(platform, "WINDOWS", True):
            component = self.prepare()
        self.assertTrue(component.is_relative_to(self.state.resolve()),
                        "Windows registered the immutable package instead of the private component")
        self.assertFalse(any(ord(character) > 0xffff for character in str(component.resolve())),
                         "The JVM canonical classpath still contains supplementary Unicode")
        self.assertEqual({name: (component / name).read_bytes() for name in INPUTS}, INPUTS)
        self.assertTrue((component / "lib" / "deductum_backend.jar").is_file())
        self.assertEqual(file_bytes(self.source), original, "Component setup modified the package")

    def test_unix_registers_and_builds_the_original_unicode_component(self):
        with patch.object(platform, "WINDOWS", False):
            component = self.prepare()
        self.assertEqual(component, self.source)
        self.assertTrue((self.source / "lib" / "deductum_backend.jar").is_file())
        self.assertFalse(self.state.exists(), "Unix unexpectedly acquired the Windows cache boundary")

    def test_windows_reuses_equal_bytes_despite_timestamps_and_generated_package_jars(self):
        with patch.object(platform, "WINDOWS", True):
            first = self.prepare()
            for name in INPUTS:
                path = self.source / name
                stamp = path.stat()
                os.utime(path, ns=(stamp.st_atime_ns, stamp.st_mtime_ns + 10_000_000_000))
            generated = self.source / "lib" / "deductum_backend.jar"
            generated.parent.mkdir()
            generated.write_bytes(b"unrelated generated package output")
            second = self.prepare()
        self.assertEqual(first, second, "Cache identity depends on mtimes or generated jars")
        self.assertEqual(self.process.builds, [first], "An unchanged component was rebuilt")
        self.assertEqual(generated.read_bytes(), b"unrelated generated package output")

    def test_windows_changes_in_any_component_input_select_a_new_cache_with_old_versions_retained(self):
        replacements = {"src/backend.scala": (b"1", b"2"),
                        "ML/deductum_prelude.ML": (b"1", b"2"),
                        "etc/settings": (b"1", b"2"),
                        "etc/build.props": (b"Test", b"Next")}
        with patch.object(platform, "WINDOWS", True):
            previous = self.prepare()
            for name, (old, new) in replacements.items():
                with self.subTest(input=name):
                    path = self.source / name
                    original = path.read_bytes()
                    changed = original.replace(old, new)
                    self.assertNotEqual(changed, original, "Invalid same-size mutation fixture")
                    self.assertEqual(len(changed), len(original))
                    stamp = path.stat()
                    path.write_bytes(changed)
                    os.utime(path, ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
                    current = self.prepare()
                    self.assertNotEqual(current, previous, f"Changed {name} reused stale compiled inputs")
                    self.assertEqual((current / name).read_bytes(), changed)
                    self.assertTrue((previous / "lib" / "deductum_backend.jar").is_file(),
                                    "Preparing a new version deleted an old component")
                    previous = current

    def test_windows_failed_compilation_can_be_retried_without_marking_the_component_ready(self):
        self.process.fail_next_build = True
        original = file_bytes(self.source)
        with patch.object(platform, "WINDOWS", True):
            with self.assertRaisesRegex(HostError, "fixture compilation failed"):
                self.prepare()
            component = self.process.component()
            self.assertFalse((component / "lib" / "deductum_backend.jar").exists())
            recovered = self.prepare()
        self.assertEqual(recovered, component)
        self.assertTrue((recovered / "lib" / "deductum_backend.jar").is_file())
        self.assertEqual(file_bytes(self.source), original)

    def test_windows_canceled_preparation_does_not_register_a_component_and_can_be_retried(self):
        cancellation = threading.Event()
        cancellation.set()
        original = file_bytes(self.source)
        with patch.object(platform, "WINDOWS", True):
            with self.assertRaises(InterruptedError):
                self.prepare(cancellation)
            self.assertEqual(self.process.entries(), [], "Canceled preparation registered a component")
            self.assertEqual(self.process.builds, [])
            cancellation.clear()
            component = self.prepare(cancellation)
        self.assertTrue((component / "lib" / "deductum_backend.jar").is_file())
        self.assertEqual(file_bytes(self.source), original)

    def test_windows_state_with_supplementary_unicode_is_refused_before_registration(self):
        invalid_state = self.root / "private 😀 state"
        with patch.object(platform, "WINDOWS", True), \
                patch.object(platform, "state_dir", return_value=invalid_state):
            with self.assertRaises(HostError) as caught:
                self.prepare()
        self.assertIn("DEDUCTUM_STATE_DIR", str(caught.exception), "No actionable state-location guidance")
        self.assertEqual(self.process.entries(), [])
        self.assertEqual(self.process.builds, [])
        self.assertFalse((self.source / "lib").exists(), "Refused preparation changed the package")

    def test_windows_migrates_verified_legacy_backends_without_discarding_unrelated_registry_lines(self):
        legacy = self.root / "old Deductum 😀" / "backend" / "Isabelle2025"
        write_component(legacy)
        (legacy / "lib").mkdir()
        (legacy / "lib" / "deductum_backend.jar").write_bytes(b"old jar")
        # A real existing directory with an 8.3-shaped spelling avoids mocking
        # component ownership. Native path tests cover actual alias identity.
        old_alias = str(self.root / "OLDDED~1" / "BACKEND" / "ISABEL~1")
        write_component(Path(old_alias))
        unrelated = self.root / "deductum-other" / "backend" / "Isabelle2025"
        (unrelated / "etc").mkdir(parents=True)
        # A matching variable name alone is insufficient ownership evidence.
        (unrelated / "etc" / "settings").write_bytes(INPUTS["etc/settings"])
        (unrelated / "etc" / "build.props").write_text("services = unrelated.Tools\n", encoding="utf-8")
        unknown_missing = str(self.root / "deductum-missing" / "backend" / "Isabelle2025")
        owned_missing = str(self.state / "isabelle-components" / "Isabelle2025" / ("a" * 64))
        unknown_cache = str(self.state / "isabelle-components" / "Isabelle2025" / "not-a-digest")
        preserved = ["# Existing components and user comments", "", "  " + str(unrelated) + "  ",
                     unknown_missing, unknown_cache, "# End user configuration"]
        self.process.registry.write_text("\n".join([*preserved[:2], str(self.source), old_alias,
                                                   str(legacy), owned_missing, *preserved[2:]]) + "\n", encoding="utf-8")
        with patch.object(platform, "WINDOWS", True):
            component = self.prepare()
        lines = self.process.registry.read_text(encoding="utf-8").splitlines()
        for line in preserved:
            self.assertIn(line, lines, "Registry migration altered an unrelated entry or comment")
        entries = self.process.entries()
        self.assertNotIn(str(self.source), entries, "The original package remains on the JVM classpath")
        self.assertNotIn(old_alias, entries, "The verified old 8.3 component alias remains registered")
        self.assertNotIn(str(legacy), entries, "The bad canonical JVM classpath remains registered")
        self.assertNotIn(owned_missing, entries, "A recognizably owned missing cache remains registered")
        self.assertIn(str(component), entries)
        self.assertTrue((legacy / "lib" / "deductum_backend.jar").is_file(),
                        "Migration deleted the old package rather than unregistering it")

    def test_windows_refuses_modified_cached_inputs_instead_of_running_or_replacing_them(self):
        with patch.object(platform, "WINDOWS", True):
            component = self.prepare()
            original_registry = self.process.registry.read_bytes()
            (component / "ML" / "deductum_prelude.ML").write_bytes(b"unexpected cached program\n")
            built = list(self.process.builds)
            with self.assertRaises(HostError):
                self.prepare()
        self.assertEqual(self.process.builds, built, "A damaged cached component was executed")
        self.assertEqual(self.process.registry.read_bytes(), original_registry)
        self.assertEqual((component / "ML" / "deductum_prelude.ML").read_bytes(),
                         b"unexpected cached program\n", "An active cache was silently replaced")

    def test_windows_interrupted_copy_never_registers_partial_inputs_and_retry_succeeds(self):
        original_write = Path.write_bytes
        injected = threading.Event()

        def interrupted_write(path, data):
            result = original_write(path, data)
            if path.is_relative_to(self.state) and data in INPUTS.values() and not injected.is_set():
                injected.set()
                raise OSError("fixture interrupted component copy")
            return result

        with patch.object(platform, "WINDOWS", True):
            with patch.object(Path, "write_bytes", interrupted_write):
                with self.assertRaisesRegex((OSError, HostError), "fixture interrupted component copy"):
                    self.prepare()
            self.assertTrue(injected.is_set(), "The fixture never interrupted a cache input write")
            self.assertEqual(self.process.entries(), [], "An interrupted copy was registered")
            self.assertEqual(self.process.builds, [])
            self.assertFalse(any(path.name in {"backend.scala", "deductum_prelude.ML", "build.props", "settings"}
                                 for path in self.state.rglob("*") if path.is_file()),
                             "An interrupted owned staging copy was left behind")
            component = self.prepare()
        self.assertEqual({name: (component / name).read_bytes() for name in INPUTS}, INPUTS)

    def test_windows_cancellation_during_copy_cleans_staging_and_allows_retry(self):
        original_write = Path.write_bytes
        cancellation = threading.Event()

        def cancel_after_write(path, data):
            result = original_write(path, data)
            if path.is_relative_to(self.state) and data in INPUTS.values():
                cancellation.set()
            return result

        with patch.object(platform, "WINDOWS", True):
            with patch.object(Path, "write_bytes", cancel_after_write):
                with self.assertRaises(InterruptedError):
                    self.prepare(cancellation)
            self.assertTrue(cancellation.is_set(), "The fixture never canceled cache population")
            self.assertEqual(self.process.entries(), [])
            self.assertEqual(self.process.builds, [])
            cancellation.clear()
            component = self.prepare(cancellation)
        self.assertTrue((component / "lib" / "deductum_backend.jar").is_file())

    def test_windows_concurrent_preparation_returns_one_complete_registered_component(self):
        original_write = Path.write_bytes
        copying = threading.Event()
        release = threading.Event()
        second_started = threading.Event()
        first_writer = threading.Lock()

        def pause_first_copy(path, data):
            result = original_write(path, data)
            if path.is_relative_to(self.state) and data in INPUTS.values() and first_writer.acquire(False):
                copying.set()
                if not release.wait(10):
                    raise AssertionError("The test did not release the component copy")
            return result

        def prepare_second():
            second_started.set()
            return self.prepare()

        with patch.object(platform, "WINDOWS", True), patch.object(Path, "write_bytes", pause_first_copy), \
                concurrent.futures.ThreadPoolExecutor(max_workers=2) as workers:
            first = workers.submit(self.prepare)
            try:
                self.assertTrue(copying.wait(10), "The first caller never reached component population")
                second = workers.submit(prepare_second)
                self.assertTrue(second_started.wait(10), "The concurrent caller never started")
            finally:
                release.set()
            first_component = first.result(timeout=10)
            second_component = second.result(timeout=10)
        self.assertEqual(first_component, second_component)
        self.assertEqual({name: (first_component / name).read_bytes() for name in INPUTS}, INPUTS)
        self.assertTrue((first_component / "lib" / "deductum_backend.jar").is_file())
        self.assertEqual(len(self.process.entries()), 1, "Concurrent registration duplicated the component")
        self.assertEqual(file_bytes(self.source), INPUTS, "Concurrent preparation changed the package")

    def test_windows_different_registries_wait_for_the_same_complete_backend_build(self):
        other_home = self.root / "other-official-runtime"
        (other_home / "etc").mkdir(parents=True)
        (other_home / "etc" / "ISABELLE_IDENTIFIER").write_text("Isabelle2025\n", encoding="utf-8")
        (other_home / "bin").mkdir()
        other_runtime = str(other_home / "bin" / "isabelle")
        other = ComponentRuntime(self.root / "other-isabelle-user")
        self.processes[other_runtime] = other
        building = threading.Event()
        release = threading.Event()
        observed_second = threading.Event()
        second_preflight = threading.Event()
        second_identity = []
        secure_tree = isabelle._component_secure_tree
        expected = b"compiled fixture jar\n"

        def partial_build(component):
            jar = component / "lib" / "deductum_backend.jar"
            jar.parent.mkdir(exist_ok=True)
            jar.write_bytes(b"partial")
            building.set()
            if not release.wait(10):
                raise AssertionError("The test did not release the first compiler")

        self.process.before_build = partial_build

        def observe_preflight(path, **kwargs):
            result = secure_tree(path, **kwargs)
            if second_identity and threading.get_ident() == second_identity[0] and kwargs.get("harden") is False:
                second_preflight.set()
            return result

        class WaitingCancellation:
            # Observe cancellation checks after the second caller's OS preflight.
            # A premature return records its partial bytes before releasing the
            # compiler instead. Neither path depends on a scheduling sleep.
            def is_set(token):
                if second_preflight.is_set():
                    observed_second.set()
                return False

        def prepare_other():
            try:
                second_identity.append(threading.get_ident())
                isabelle.ensure_component(other_runtime, self.progress.append,
                                          cancel_event=WaitingCancellation())
                component = other.component()
                self.assertEqual((component / "lib" / "deductum_backend.jar").read_bytes(), expected,
                                 "A second registry reused the first compiler's incomplete jar")
                return component
            finally:
                observed_second.set()

        with patch.object(platform, "WINDOWS", True), \
                patch.object(isabelle, "_component_secure_tree", side_effect=observe_preflight), \
                concurrent.futures.ThreadPoolExecutor(max_workers=2) as workers:
            first = workers.submit(self.prepare)
            try:
                self.assertTrue(building.wait(10), "The first compiler never exposed its partial jar")
                second = workers.submit(prepare_other)
                self.assertTrue(observed_second.wait(10), "The second caller made no observable progress")
            finally:
                release.set()
            component = first.result(timeout=10)
            other_component = second.result(timeout=10)
        self.assertEqual(component, other_component, "Equal inputs did not share the content cache")
        self.assertEqual(self.process.builds + other.builds, [component],
                         "Different registries compiled the same shared component concurrently")

    def test_windows_modified_accepted_jar_is_refused_without_execution_or_cache_mutation(self):
        with patch.object(platform, "WINDOWS", True):
            component = self.prepare()
            jar = component / "lib" / "deductum_backend.jar"
            expected = jar.read_bytes()
            stamp = jar.stat()
            jar.write_bytes(b"x" * len(expected))
            os.utime(jar, ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
            damaged = file_bytes(component)
            registry = self.process.registry.read_bytes()
            with self.assertRaises(HostError) as caught:
                self.prepare()
        self.assertIn("DEDUCTUM_STATE_DIR", str(caught.exception), "No actionable cache recovery guidance")
        self.assertEqual(file_bytes(component), damaged, "An accepted damaged cache was mutated or blessed")
        self.assertEqual(self.process.registry.read_bytes(), registry)
        self.assertEqual(self.process.builds, [component], "The incremental compiler was run on accepted corruption")

    def test_windows_unaccepted_partial_jar_is_removed_before_incremental_compilation(self):
        with patch.object(platform, "WINDOWS", True):
            component = self.prepare()
            jar = component / "lib" / "deductum_backend.jar"
            expected = jar.read_bytes()
            receipt = jar.with_name("_deductum-build.sha256")
            receipt.unlink()
            jar.write_bytes(b"partial jar with an unchanged embedded source checksum")
            unrelated = jar.with_name("unrelated.jar")
            unrelated.write_bytes(b"preserve another component's output")
            recovered = self.prepare()
        self.assertEqual(recovered, component)
        self.assertEqual(jar.read_bytes(), expected,
                         "An unaccepted jar survived the incremental compiler's no-op")
        self.assertTrue(receipt.is_file(), "The successful rebuilt jar was not accepted")
        self.assertEqual(self.process.builds, [component, component])
        self.assertEqual(unrelated.read_bytes(), b"preserve another component's output")

    def test_windows_relative_registry_entries_use_the_isabelle_home_instead_of_the_host_cwd(self):
        relative = "deductum-other/backend/Isabelle2025"
        working = self.root / "host-working-directory"
        write_component(working / relative)
        unrelated = self.process.home_user / relative
        (unrelated / "etc").mkdir(parents=True)
        (unrelated / "etc" / "settings").write_text("UNRELATED_COMPONENT=1\n", encoding="utf-8")
        self.process.registry.write_text(relative + "\n", encoding="utf-8")
        original_cwd = Path.cwd()
        try:
            os.chdir(working)
            with patch.object(platform, "WINDOWS", True):
                self.prepare()
        finally:
            os.chdir(original_cwd)
        self.assertIn(relative, self.process.entries(),
                      "An unrelated home-relative component was mistaken for an owned cwd-relative path")
        self.assertTrue((working / relative / "src" / "backend.scala").is_file())

    def test_windows_current_component_is_registered_using_the_selected_ascii_alias(self):
        self.state = self.root / "private β state"
        with patch.object(platform, "WINDOWS", True), patch.object(platform, "state_dir", return_value=self.state):
            component = self.prepare()
            alias = str(self.root / "CACHE~1" / component.name)
            self.process.native_paths[alias] = component
            self.process.cli_paths[str(component)] = alias
            self.process.registry.write_text("# Keep this comment\n" + str(component) + "\n", encoding="utf-8")
            selected = self.prepare()
        self.assertEqual(selected, component)
        self.assertEqual(self.process.entries(), [alias],
                         "An existing canonical Unicode entry bypassed the ASCII CLI alias")
        self.assertIn("# Keep this comment", self.process.registry.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
