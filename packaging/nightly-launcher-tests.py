#!/usr/bin/env python3
"""Synthetic contract for the Linux nightly launchers: no native build or executable launch.

The unpacked tar's `designcraft` is craft-common-actions's canonical launcher template
(scripts/nightly-launcher.sh.in) rendered for this app, which the shared nightly producer accepts
byte for byte; `TarTemplate` pins its exact argv. Every distro format (deb/rpm wrapper, AppImage
AppRun, Arch launcher, Flatpak wrapper) must start the compiled nightly-v1 flavor the Launcher's
way, because the app refuses a DESIGNCRAFT_CONFIG_DIR override and refuses a bare start as stable.
"""
import os
import re
import shutil
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
    }


TAR_LAUNCHER = LINUX / "nightly-launch.sh"
TAR_IDENTITY = "ai.storyteller.designcraft.nightly"
RECORDER = '#!/bin/sh\nfor arg in "$@"; do printf "<%s>\\n" "$arg"; done\n'


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
        self.assertIn('install -m755 "$ROOT/packaging/linux/nightly-launch.sh" "$STAGE/designcraft"', nightly,
                      "the tar installs the canonical template, not its own copy of the protocol")

    def test_every_format_passes_an_existing_profile_pair_through(self):
        for label, lines in launchers().items():
            with self.subTest(launcher=label):
                self.assertEqual(sum(line == PASSTHROUGH for line in lines), 1, "a caller's own profile pair is never doubled")

class TarTemplate(unittest.TestCase):
    """The plain nightly tar's launcher: the canonical shared template, byte for byte."""

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name).resolve()
        self.tree = self.root / "unpacked"
        self.tree.mkdir()
        self.program = self.tree / "designcraft-bin"
        self.program.write_text(RECORDER, encoding="utf-8")
        self.program.chmod(0o755)
        self.command = self.tree / "designcraft"
        shutil.copyfile(TAR_LAUNCHER, self.command)
        self.command.chmod(0o755)
        self.home = self.root / "home with space"
        self.home.mkdir()

    def execute(self, *arguments, env=None):
        environment = {"HOME": str(self.home), "PATH": "/usr/bin:/bin", **(env or {})}
        environment = {key: value for key, value in environment.items() if value is not None}
        return subprocess.run([str(self.command), *arguments], cwd="/", env=environment,
                              capture_output=True, text=True, timeout=15)

    def argv(self, *arguments, env=None):
        run = self.execute(*arguments, env=env)
        self.assertEqual(run.returncode, 0, run.stderr)
        return run.stdout.splitlines()

    def profile(self, data_home=None):
        return str(Path(data_home or self.home / ".local/share") / TAR_IDENTITY)

    def test_the_file_is_the_rendered_template(self):
        source = TAR_LAUNCHER.read_text(encoding="utf-8")
        self.assertIn("# Nightly launcher generated by craft-common-actions scripts/nightly-launcher.sh.in.", source)
        self.assertIn('target="$dir/designcraft-bin"', source)
        self.assertIn('  --launch-profile) exec "$target" "$@" ;;', source)
        self.assertIn(f'profile_root="${{XDG_DATA_HOME:-$HOME/.local/share}}/{TAR_IDENTITY}"', source)
        for placeholder in ("@BINARY@", "@APP_ID@"):
            self.assertNotIn(placeholder, source)

    def test_a_launch_adds_the_profile_pair_and_the_files_follow_the_delimiter(self):
        files = ["doc with spaces.dc", "--other-name.dc"]
        self.assertEqual(self.argv(*files),
                         ["<--launch-profile>", "<nightly-v1>", "<--profile-root>", f"<{self.profile()}>", "<-->",
                          *[f"<{name}>" for name in files]])
        self.assertEqual(self.argv(),
                         ["<--launch-profile>", "<nightly-v1>", "<--profile-root>", f"<{self.profile()}>", "<-->"])

    def test_the_profile_follows_xdg_data_home(self):
        data_home = self.root / "xdg with space"
        self.assertEqual(self.argv("doc.dc", env={"XDG_DATA_HOME": str(data_home)})[3],
                         f"<{self.profile(data_home)}>")

    def test_both_version_spellings_ask_for_one_plain_version(self):
        for flag in ("--version", "-V"):
            with self.subTest(flag=flag):
                self.assertEqual(self.argv(flag),
                                 ["<--launch-profile>", "<nightly-v1>", "<--profile-root>",
                                  f"<{self.profile()}>", "<--version>"])

    def test_the_launchers_own_profile_line_passes_through_untouched(self):
        # The ArtCraft Launcher starts the tar's entry point with its own profile pair and per-install
        # root; the template hands that line to the GUI unchanged.
        managed = "/launcher/root/designcraft"
        for supplied in (["--launch-profile", "nightly-v1", "--profile-root", managed, "--"],
                         ["--launch-profile", "nightly-v1", "--profile-root", managed, "--", "doc.dc"],
                         ["--launch-profile"]):
            with self.subTest(supplied=supplied):
                self.assertEqual(self.argv(*supplied), [f"<{name}>" for name in supplied])

    def test_every_other_line_still_gets_the_templates_own_profile(self):
        # Only a line that opens with `--launch-profile` exactly is the Launcher's.
        for arguments in (["--profile-root", "/launcher/root/designcraft", "--"],
                          ["--launch-profile=nightly-v1"], ["--help"]):
            with self.subTest(arguments=arguments):
                self.assertEqual(self.argv(*arguments),
                                 ["<--launch-profile>", "<nightly-v1>", "<--profile-root>", f"<{self.profile()}>", "<-->",
                                  *[f"<{name}>" for name in arguments]])

    def test_it_starts_only_the_binary_beside_it(self):
        self.program.unlink()
        for arguments in ((), ("--version",), ("doc.dc",)):
            with self.subTest(arguments=arguments):
                run = self.execute(*arguments)
                self.assertNotEqual(run.returncode, 0, run.stdout)
                self.assertEqual(run.stdout, "")

    def test_no_data_folder_means_no_launch(self):
        run = self.execute("doc.dc", env={"HOME": None})
        self.assertNotEqual(run.returncode, 0)
        self.assertEqual(run.stdout, "")
        self.assertIn("HOME", run.stderr)


if __name__ == "__main__":
    unittest.main()
