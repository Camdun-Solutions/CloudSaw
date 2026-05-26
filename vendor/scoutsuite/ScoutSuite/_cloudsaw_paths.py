# PyInstaller-aware path helpers — CloudSaw fork patch.
#
# ScoutSuite's upstream source walks `__file__` to locate sibling data files
# (rule JSON, per-provider metadata.json, HTML report templates). Under
# `pyinstaller --onefile`, the interpreter rewrites `__file__` to point inside
# the runtime extraction directory (`sys._MEIPASS`), and the same walks would
# in principle keep working — as long as the data files were declared in the
# spec's `datas` list.
#
# Two reasons we still consolidate the walks here:
#
#   1. The stock code mixes `os.path.realpath(__file__)` and
#      `os.path.abspath(__file__)`. `realpath` follows symlinks; PyInstaller's
#      `_MEIPASS` directory sometimes contains symlinks (notably on macOS when
#      the bundle is extracted under `$TMPDIR`), and `realpath` can lead the
#      walk OUTSIDE `_MEIPASS` to the staging path the launcher used. Using
#      `abspath` everywhere keeps the walks inside the bundle.
#
#   2. Having a single named call site per "walk pattern" gives the PyInstaller
#      spec maintainer (`vendor/scoutsuite/cloudsaw.spec`) one place to look
#      when a frozen build fails with FileNotFoundError — the helper signature
#      names the relationship between `__file__` and the data path.
#
# Both helpers take `__file__` as an argument rather than capturing it via a
# stack frame so they're cheap, testable, and work identically frozen and
# unfrozen. Default behavior under regular CPython is byte-identical to the
# `abspath` variants the upstream code already uses.

from __future__ import annotations

import os
import sys


def _is_frozen() -> bool:
    """True when running inside a PyInstaller bundle.

    PyInstaller sets both `sys.frozen` and `sys._MEIPASS` on the
    interpreter. Other freezing tools (Nuitka, py2exe, cx_Freeze) may set
    only one or use different attribute names — extend this check if we
    ever swap freezers.
    """
    return getattr(sys, "frozen", False) and hasattr(sys, "_MEIPASS")


def package_dir(module_file: str) -> str:
    """Return the directory containing `module_file`.

    Drop-in replacement for the upstream idiom
    ``os.path.dirname(os.path.abspath(__file__))`` / ``os.path.realpath``.
    """
    return os.path.dirname(os.path.abspath(module_file))


def package_parent(module_file: str) -> str:
    """Return the parent of the directory containing `module_file`.

    Drop-in replacement for the upstream idiom
    ``os.path.dirname(os.path.dirname(os.path.abspath(__file__)))``, used by
    `ScoutSuite/core/ruleset.py` to walk up from `ScoutSuite/core/` to the
    `ScoutSuite/` package root before joining `providers/<cloud>/rules`.
    """
    return os.path.dirname(os.path.dirname(os.path.abspath(module_file)))
