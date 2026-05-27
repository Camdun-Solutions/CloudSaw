# -*- mode: python ; coding: utf-8 -*-
#
# PyInstaller spec for the CloudSaw bundled ScoutSuite binary.
#
# The spec is a Python file PyInstaller consumes; everything below is
# evaluated by PyInstaller's own runtime (the syntax warnings in your
# editor about undefined `Analysis`/`PYZ`/`EXE` symbols are expected —
# they're injected by PyInstaller at evaluation time).
#
# Maintainer notes:
#   * Keep `name='scoutsuite'` in the EXE block. CloudSaw's
#     `src-tauri/src/scanner/binary.rs::locate()` searches for exactly
#     this filename under `vendor/scoutsuite/<triple>/`.
#   * UPX is OFF intentionally. UPX-packed binaries trip Windows
#     Defender false-positive heuristics, and the size savings aren't
#     worth the install-time friction.
#   * Plugin-discovery patches (Layer A of the implementation plan)
#     are in vendor/scoutsuite/ScoutSuite/_cloudsaw_paths.py and route
#     `__file__` walks through `package_dir()`/`package_parent()`.
#     That keeps the walks PyInstaller-aware. But the patches alone are
#     not enough — the data files those walks land on still have to be
#     declared below in `datas`.

import glob
import os

from PyInstaller.utils.hooks import collect_data_files, collect_submodules

block_cipher = None

# Initial best-effort `datas` — refined in vendor/scoutsuite/SPEC_NOTES.md
# as the iteration loop surfaces missing files.
#
# Tuple format: ('source-pattern-on-disk', 'destination-relpath-in-bundle').
# Glob patterns DO work in `datas` — PyInstaller expands them at spec eval.
datas = [
    # Per-provider security rule JSON. CloudSaw only exercises AWS today
    # (Contract 06 is AWS-only) but bundling the others is cheap and
    # keeps the spec uniform; if/when CloudSaw adds GCP/Azure scanning
    # we don't need to re-tune.
    ('ScoutSuite/providers/aws/rules', 'ScoutSuite/providers/aws/rules'),
    ('ScoutSuite/providers/azure/rules', 'ScoutSuite/providers/azure/rules'),
    ('ScoutSuite/providers/gcp/rules', 'ScoutSuite/providers/gcp/rules'),
    ('ScoutSuite/providers/aliyun/rules', 'ScoutSuite/providers/aliyun/rules'),
    ('ScoutSuite/providers/oci/rules', 'ScoutSuite/providers/oci/rules'),
    ('ScoutSuite/providers/do/rules', 'ScoutSuite/providers/do/rules'),
    ('ScoutSuite/providers/kubernetes/rules', 'ScoutSuite/providers/kubernetes/rules'),

    # Per-provider metadata.json — walked by the provider's __init__ via
    # the patched package_dir() helper.
    ('ScoutSuite/providers/aws/metadata.json', 'ScoutSuite/providers/aws'),
    ('ScoutSuite/providers/azure/metadata.json', 'ScoutSuite/providers/azure'),
    ('ScoutSuite/providers/gcp/metadata.json', 'ScoutSuite/providers/gcp'),
    ('ScoutSuite/providers/aliyun/metadata.json', 'ScoutSuite/providers/aliyun'),
    ('ScoutSuite/providers/oci/metadata.json', 'ScoutSuite/providers/oci'),
    ('ScoutSuite/providers/do/metadata.json', 'ScoutSuite/providers/do'),
    ('ScoutSuite/providers/kubernetes/metadata.json', 'ScoutSuite/providers/kubernetes'),

    # HTML report templates + embedded JS/CSS used by `ScoutSuite/output/html.py`.
    # (`ScoutSuite/output/template` does NOT exist upstream — templates live
    # inside `output/data/html/` instead.)
    ('ScoutSuite/output/data', 'ScoutSuite/output/data'),

    # Static reference data (ip-ranges.json, etc.) under ScoutSuite/data/.
    # ScoutSuite/core/fs.py walks `../data/` from `core/`, so we mirror
    # that layout.
    #
    # IMPORTANT: enumerated explicitly rather than using the directory
    # tuple form `('ScoutSuite/data', 'ScoutSuite/data')`. PyInstaller's
    # directory-tuple semantics are platform-inconsistent — the Windows
    # bundle in 2026.5.9-2026.5.13 picked up the entire subtree, but the
    # macOS CI build silently dropped `icmp_message_types.json` and
    # `protocols.json`, surfacing as a runtime `[Errno 2] No such file or
    # directory: '.../ScoutSuite/core/../data/icmp_message_types.json'`
    # at scan-time once the AWSProvider class definition loaded
    # SecurityGroups. Explicit globs remove the ambiguity.
]
for root, _dirs, files in os.walk('ScoutSuite/data'):
    for fname in files:
        src = os.path.join(root, fname).replace('\\', '/')
        # Destination path inside the bundle = source's directory, with
        # forward-slash normalization so the bundle layout matches what
        # ScoutSuite's `core/fs.py` expects on every platform.
        dest = root.replace('\\', '/')
        datas.append((src, dest))
# Fail-fast assertions: if the source tree changes shape we want a build-time
# error in CI, not another runtime-only failure on a user's install. These
# specific files have a history of going missing on macOS bundles via the
# directory-tuple form (see SPEC_NOTES.md 2026-05-27).
assert any('icmp_message_types.json' in p for p, _ in datas), \
    'icmp_message_types.json missing from datas — check vendor/scoutsuite/ScoutSuite/data/'
assert any('protocols.json' in p for p, _ in datas), \
    'protocols.json missing from datas — check vendor/scoutsuite/ScoutSuite/data/'
assert any('ip-ranges/aws.json' in p for p, _ in datas), \
    'ip-ranges/aws.json missing from datas — check vendor/scoutsuite/ScoutSuite/data/aws/ip-ranges/'

# botocore + boto3 ship cloud service models as JSON under their own
# `data/` directories. Without these, the first AWS API call raises
# `DataNotFoundError` at runtime. `collect_data_files` is PyInstaller's
# canonical way to pull in a package's data tree; safer than writing
# the glob ourselves because boto3/botocore release cadence is high
# and the directory structure occasionally changes.
datas += collect_data_files('botocore')
datas += collect_data_files('boto3')

# CherryPy ships HTML/JS for its web UI — ScoutSuite uses CherryPy to
# serve the local report viewer. Without these the report serve path
# (`-u` / `--no-browser` off) crashes; with `--no-browser` (which
# CloudSaw uses) this MAY not be strictly required, but the cost is
# small and the future-proofing is worth it.
datas += collect_data_files('cherrypy')

# policyuniverse — Netflix's AWS IAM policy parser. Ships a `data.json`
# alongside its `__init__.py`; without it the import fails at scan time
# before any AWS API call. ScoutSuite uses it from
# `ScoutSuite/core/conditions.py`.
datas += collect_data_files('policyuniverse')

# Initial best-effort `hiddenimports`. ScoutSuite's __main__ dispatches
# providers via string→module lookup; PyInstaller's static analyzer
# misses some. We declare the per-provider package roots so the
# analyzer follows the entire provider subtree.
hiddenimports = []
hiddenimports += collect_submodules('ScoutSuite.providers.aws')
hiddenimports += collect_submodules('ScoutSuite.providers.azure')
hiddenimports += collect_submodules('ScoutSuite.providers.gcp')
hiddenimports += collect_submodules('ScoutSuite.providers.aliyun')
hiddenimports += collect_submodules('ScoutSuite.providers.oci')
hiddenimports += collect_submodules('ScoutSuite.providers.do')
hiddenimports += collect_submodules('ScoutSuite.providers.kubernetes')

# Common boto3/botocore extension paths PyInstaller often misses:
hiddenimports += [
    'botocore.vendored.requests.packages.urllib3.contrib.pyopenssl',
    'botocore.compat',
    'botocore.handlers',
    'botocore.client',
]

# Per-provider authentication_strategy modules — `__import__`'d as a string
# by `ScoutSuite/providers/base/authentication_strategy_factory.py:14`,
# which `collect_submodules` doesn't pick up reliably. Declared explicitly
# so the frozen binary can satisfy the dynamic import at scan startup.
hiddenimports += [
    'ScoutSuite.providers.aws.authentication_strategy',
    'ScoutSuite.providers.azure.authentication_strategy',
    'ScoutSuite.providers.gcp.authentication_strategy',
    'ScoutSuite.providers.aliyun.authentication_strategy',
    'ScoutSuite.providers.oci.authentication_strategy',
    'ScoutSuite.providers.do.authentication_strategy',
    'ScoutSuite.providers.kubernetes.authentication_strategy',
]

# Per-provider provider modules — `__import__`'d as a string by
# `ScoutSuite/providers/__init__.py:12` (`get_provider_object()`):
#
#     provider_module = __import__(
#         f'ScoutSuite.providers.{provider}.provider',
#         fromlist=[provider_class]
#     )
#
# Same dynamic-dispatch pattern as authentication_strategy_factory above —
# PyInstaller's static analyzer can't follow the f-string, and
# `collect_submodules('ScoutSuite.providers.aws')` silently drops the
# concrete `.provider` submodule from the freeze (the cause appears to
# be `provider.py`'s transitive import surface, but the symmetric fix
# is to declare it explicitly the same way we did for
# `authentication_strategy`).
#
# 2026.5.9-2026.5.12 shipped without these entries; the auth strategy
# resolved fine, so scout.py printed "Authenticating to cloud provider"
# successfully — but the very next step (`get_provider_object('aws')`)
# threw `ModuleNotFoundError: No module named 'ScoutSuite.providers.aws.provider'`
# at runtime. Our fake-credentials Phase 1 validation never reached this
# code path because AWS rejected the fake STS token first.
hiddenimports += [
    'ScoutSuite.providers.aws.provider',
    'ScoutSuite.providers.azure.provider',
    'ScoutSuite.providers.gcp.provider',
    'ScoutSuite.providers.aliyun.provider',
    'ScoutSuite.providers.oci.provider',
    'ScoutSuite.providers.do.provider',
    'ScoutSuite.providers.kubernetes.provider',
]

a = Analysis(
    ['scout.py'],
    pathex=['.'],
    binaries=[],
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        # ScoutSuite vendors a handful of optional reporting deps that
        # PyInstaller likes to drag in. Exclude what we don't need to
        # keep the binary smaller. Conservative list — add to it only
        # when iteration shows a clear "X is imported but never used".
    ],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
)
pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.zipfiles,
    a.datas,
    [],
    name='scoutsuite',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,           # see comment at top — UPX off intentionally
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,        # ScoutSuite is a CLI; stays console-bound
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,  # macOS codesign happens in release.yml after PyInstaller
    entitlements_file=None,
)
