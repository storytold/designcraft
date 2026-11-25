#!/usr/bin/env python3
"""Synthetic contract for the Linux nightly launchers: no native build or executable launch.

Every Linux nightly format (deb/rpm wrapper, AppImage AppRun, Arch launcher, Flatpak wrapper and
the unpacked tar) must start the compiled nightly-v1 flavor the Launcher's way, because the app
refuses a DESIGNCRAFT_CONFIG_DIR override and refuses a bare start as stable.
"""
import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LINUX = ROOT / "packaging/linux"
CHANNEL = ROOT / "crates/ui-egui/src/channel.rs"

EXEC = 'exec "$real_bin" --launch-profile nightly-v1 --profile-root "$profile_root" -- "$@"'
PROFILE_ROOT = 'profile_root="${XDG_DATA_HOME:-${HOME:?HOME is required}/.local/share}/designcraft-nightly"'
PASSTHROUGH = 'if [ "${1:-}" = "--launch-profile" ] || [ "${1:-}" = "--profile-root" ]; then'


def body(path, marker):
    lines = (ROOT / path).read_text(encoding="utf-8").splitlines()
    opening = [index for index, line in enumerate(lines) if f"<<'{marker}'" in line]
    if len(opening) != 1:
        raise AssertionError(f"{path}: expected exactly one <<'{marker}' here-document")
    end = lines.index(marker, opening[0] + 1)
    return lines[opening[0] + 1:end]


def launchers():
    """(label, the launcher's own lines) for every Linux nightly start path."""
    return {
        "deb/rpm wrapper": body("packaging/linux/package.sh", "WRAPPER"),
        "AppImage AppRun": body("packaging/linux/package.sh", "APPRUN"),
        "Arch launcher": (LINUX / "nightly-arch-launch.sh").read_text(encoding="utf-8").splitlines(),
        "Flatpak wrapper": body("packaging/linux/flatpak-bundle.sh", "WRAPPER"),
        "tar launcher": body("packaging/linux/nightly.sh", "LAUNCHER"),
    }


def tar_launcher(scratch):
    """The tar's launcher beside a recorder that prints its argv, one argument per line."""
    directory = Path(scratch)
    launcher = directory / "designcraft"
    launcher.write_text("\n".join(body("packaging/linux/nightly.sh", "LAUNCHER")) + "\n", encoding="utf-8")
    launcher.chmod(0o755)
    recorder = directory / "designcraft-bin"
    recorder.write_text('#!/bin/sh\nfor arg in "$@"; do printf "<%s>\\n" "$arg"; done\n', encoding="utf-8")
    recorder.chmod(0o755)
    return directory, launcher


def constant(name):
    source = CHANNEL.read_text(encoding="utf-8")
    found = re.search(r'pub const ' + name + r': &str = "([^"]+)";', source)
    if found is None:
        raise AssertionError(f"channel.rs has no {name}")
    return found.group(1)


class LinuxLaunchers(unittest.TestCase):
    def test_every_format_execs_the_launcher_pair(self):
        for label, lines in launchers().items():
            with self.subTest(launcher=label):
                self.assertIn(EXEC, lines)
                self.assertEqual(sum(line == EXEC for line in lines), 1, "one start path per format")

    def test_no_format_redirects_the_profile_through_the_stable_override(self):
        for label, lines in launchers().items():
            with self.subTest(launcher=label):
                redirected = [line for line in lines if not line.lstrip().startswith("#") and re.match(r"\s*(export\s+)?DESIGNCRAFT_CONFIG_DIR=", line)]
                self.assertEqual(redirected, [], "a non-empty override is refused by the app")

    def test_the_version_query_is_answered_before_the_profile_prelude(self):
        for label, lines in launchers().items():
            with self.subTest(launcher=label):
                query = next(index for index, line in enumerate(lines) if '= "--version"' in line and '= "-V"' in line)
                self.assertLess(query, lines.index(EXEC), "a headless runner asks for the version first")

    def test_every_format_uses_the_same_absolute_channel_profile(self):
        for label, lines in launchers().items():
            with self.subTest(launcher=label):
                self.assertIn(PROFILE_ROOT, lines)

    def test_the_nightly_window_id_is_the_packaged_desktop_class(self):
        app_id = constant("NIGHTLY_APP_ID")
        package = (LINUX / "package.sh").read_text(encoding="utf-8")
        flatpak = (LINUX / "flatpak-bundle.sh").read_text(encoding="utf-8")
        self.assertIn(f"StartupWMClass={app_id}", package)
        self.assertIn(f"StartupWMClass={app_id}", flatpak)
        # The stable entry keeps its own class, so the two windows never share one.
        stable = (LINUX / "ai.storyteller.designcraft.desktop").read_text(encoding="utf-8")
        self.assertIn(f"StartupWMClass={constant('APP_ID')}\n", stable)

    def test_the_flatpak_bundle_id_is_the_nightly_package_id(self):
        flatpak = (LINUX / "flatpak-bundle.sh").read_text(encoding="utf-8")
        self.assertIn('FLATPAK_ID="$APP_ID.nightly"', flatpak)
        self.assertEqual(f"{constant('APP_ID')}.nightly", constant("NIGHTLY_PACKAGE_ID"))

    def test_the_tar_archive_carries_the_launcher_beside_its_binary(self):
        policy = (ROOT / "packaging/nightly-archive.py").read_text(encoding="utf-8")
        self.assertIn('{"designcraft", "designcraft-bin", "designcraft-nightly.png"}', policy)
        self.assertIn('"designcraft-bin", "designcraft-nightly.png", "README.md"', policy)
        nightly = (LINUX / "nightly.sh").read_text(encoding="utf-8")
        self.assertIn('cp "$NIGHTLY_GUI" "$STAGE/designcraft-bin"', nightly)

    def test_every_format_passes_an_existing_profile_pair_through(self):
        for label, lines in launchers().items():
            with self.subTest(launcher=label):
                self.assertEqual(sum(line == PASSTHROUGH for line in lines), 1, "a caller's own profile pair is never doubled")

    def test_the_tar_launcher_passes_an_existing_profile_pair_through_untouched(self):
        with tempfile.TemporaryDirectory() as scratch:
            directory, launcher = tar_launcher(scratch)
            supplied = ["--launch-profile", "nightly-v1", "--profile-root", "/launcher/root/designcraft", "--", "doc.dc", "--version"]
            run = subprocess.run(
                [str(launcher), *supplied], cwd="/", env={"HOME": str(directory), "PATH": "/usr/bin:/bin"}, capture_output=True, text=True, timeout=15
            )
            self.assertEqual(run.returncode, 0, run.stderr)
            self.assertEqual(run.stdout.splitlines(), [f"<{arg}>" for arg in supplied], "the Launcher's protocol reached the binary unchanged")

    def test_the_tar_launcher_never_adds_a_second_separator(self):
        with tempfile.TemporaryDirectory() as scratch:
            directory, launcher = tar_launcher(scratch)
            profile = directory / ".local/share/designcraft-nightly"
            run = subprocess.run(
                [str(launcher), "--", "--version"], cwd="/", env={"HOME": str(directory), "PATH": "/usr/bin:/bin"}, capture_output=True, text=True, timeout=15
            )
            self.assertEqual(run.returncode, 0, run.stderr)
            self.assertEqual(
                run.stdout.splitlines(),
                ["<--launch-profile>", "<nightly-v1>", "<--profile-root>", f"<{profile}>", "<-->", "<--version>"],
                "the caller's separator stays the only one, and a flag after it is a document",
            )

    def test_the_tar_launcher_starts_its_binary_with_the_profile_pair(self):
        lines = body("packaging/linux/nightly.sh", "LAUNCHER")
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            launcher = directory / "designcraft"
            launcher.write_text("\n".join(lines) + "\n", encoding="utf-8")
            launcher.chmod(0o755)
            recorder = directory / "designcraft-bin"
            recorder.write_text('#!/bin/sh\nfor arg in "$@"; do printf "<%s>\\n" "$arg"; done\n', encoding="utf-8")
            recorder.chmod(0o755)
            environment = {"HOME": str(directory), "PATH": "/usr/bin:/bin"}
            run = subprocess.run([str(launcher), "doc with spaces.dc", "--other-name.dc"], cwd="/", env=environment, capture_output=True, text=True, timeout=15)
            self.assertEqual(run.returncode, 0, run.stderr)
            self.assertEqual(
                run.stdout.splitlines(),
                [
                    "<--launch-profile>",
                    "<nightly-v1>",
                    "<--profile-root>",
                    f"<{directory / '.local/share/designcraft-nightly'}>",
                    "<-->",
                    "<doc with spaces.dc>",
                    "<--other-name.dc>",
                ],
            )
            query = subprocess.run([str(launcher), "--version"], cwd="/", env=environment, capture_output=True, text=True, timeout=15)
            self.assertEqual(query.stdout.splitlines(), ["<--version>"], "a version query reaches the binary untouched")


if __name__ == "__main__":
    unittest.main()
