#!/usr/bin/env sh
# Stub for `npm install --prefix <dir> <pkg>@<ver>` used by chum-install's
# integration tests. Simulates a successful install without requiring a
# real npm on PATH: parses the --prefix flag, creates an empty
# node_modules/ directory under it, exits 0.
#
# Invoked via `/bin/sh tests/fixtures/fake-npm.sh ...` so it does not
# need the executable bit set (which cargo's git checkout would not
# preserve everywhere).

prefix=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --prefix)
            prefix="$2"
            shift 2
            ;;
        *)
            shift
            ;;
    esac
done

if [ -n "$prefix" ]; then
    mkdir -p "$prefix/node_modules"
fi
exit 0
