#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

# Graphify 0.9.66 otherwise re-execs Nix's shell wrapper as Python.
# Pinning the seed before startup also makes clustering deterministic.
export PYTHONHASHSEED=0
export GRAPHIFY_MAX_WORKERS="${GRAPHIFY_MAX_WORKERS:-4}"
exec graphify "$@"
