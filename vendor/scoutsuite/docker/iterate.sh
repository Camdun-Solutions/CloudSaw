#!/usr/bin/env bash
# Local PyInstaller iteration loop for the CloudSaw bundled ScoutSuite
# binary.
#
# Edit cloudsaw.spec, run this script. The script:
#   1. (Re)builds the build-environment image if Dockerfile-pyinstaller
#      or one of the pip requirements files changed.
#   2. Runs PyInstaller against the mounted source tree, producing
#      dist/scoutsuite.
#   3. Smokes the resulting binary with --help (validates entry point
#      + general datas) and aws --help (validates the AWS provider's
#      rules data + boto3 init — the critical path for CloudSaw).
#
# When both smokes pass on Linux, port the spec to macOS and Windows
# (see Layer C-prime in the implementation plan). When all three are
# clean, wire the same flow into release.yml (Layer C).

set -euo pipefail

cd "$(dirname "$0")/.."  # vendor/scoutsuite/

IMAGE="cloudsaw-scoutsuite-pyi"

# On Git Bash for Windows, MSYS auto-converts forward-slash paths into
# Windows paths when they look like CLI args ("/scoutsuite" → "C:\Program
# Files\Git\scoutsuite"). Setting MSYS_NO_PATHCONV=1 disables the
# conversion for the docker run invocations below — without this, the
# `-v <host>:/scoutsuite` mount and the `--entrypoint /scoutsuite/...`
# paths get mangled before docker ever sees them. Harmless on Linux/macOS
# (the variable is unknown there) so we always set it.
export MSYS_NO_PATHCONV=1

echo "==> Building/refreshing PyInstaller build image..."
docker build -t "$IMAGE" -f docker/Dockerfile-pyinstaller .

# Resolve the host source dir to whatever the runtime shell agrees with
# Docker on. On Git Bash, $(pwd) returns `/c/...` — Docker Desktop maps
# that fine. On native Linux/macOS shells it's just the absolute path.
HOST_DIR="$(pwd)"

echo
echo "==> Running PyInstaller against mounted source tree..."
docker run --rm -v "${HOST_DIR}:/scoutsuite" "$IMAGE"

if [ ! -f dist/scoutsuite ]; then
    echo "ERROR: PyInstaller succeeded but dist/scoutsuite is missing."
    echo "Check cloudsaw.spec's EXE() block — name= must be 'scoutsuite'."
    exit 1
fi

echo
echo "==> Smoke test 1: dist/scoutsuite --help"
docker run --rm -v "${HOST_DIR}:/scoutsuite" --entrypoint /scoutsuite/dist/scoutsuite "$IMAGE" --help

echo
echo "==> Smoke test 2: dist/scoutsuite aws --help"
docker run --rm -v "${HOST_DIR}:/scoutsuite" --entrypoint /scoutsuite/dist/scoutsuite "$IMAGE" aws --help

echo
echo "==> Both smokes passed. Binary: $(pwd)/dist/scoutsuite"
echo "    Next steps:"
echo "      * Hand-run a real scan against a sandbox AWS account."
echo "      * Port cloudsaw.spec to macOS + Windows hosts."
echo "      * Wire the same flow into .github/workflows/release.yml."
