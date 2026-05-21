#!/usr/bin/env bash
# Create the local-source directory referenced by
# `crates/chum-cli/tests/fixtures/chum-local-runnable.toml`.
#
# Idempotent — safe to run repeatedly. Used by humans driving the
# example by hand; the integration test uses its own tempdir.

set -euo pipefail

target="/tmp/chum-local-test-src"
mkdir -p "$target"
printf "chum local-source fixture sentinel\n" > "$target/SENTINEL"
echo "ready: $target"
