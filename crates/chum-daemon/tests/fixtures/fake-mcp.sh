#!/bin/sh
# Fake MCP server for chum-daemon Supervisor integration tests.
#
# POSIX shell only (no bashisms). Tested with `sh -n`. Supports the
# minimum of flags needed to drive every supervisor scenario without
# spawning a real MCP server.
#
# Usage:
#   fake-mcp.sh [--exit-code N]
#               [--exit-after-secs N]
#               [--print-to-stdout MSG]
#               [--print-to-stderr MSG]
#               [--ignore-sigterm]
#
# Defaults: exit 0 immediately.

exit_code=0
exit_after=0
stdout_msg=""
stderr_msg=""
ignore_sigterm=0

while [ $# -gt 0 ]; do
    case "$1" in
        --exit-code)
            exit_code="$2"
            shift 2
            ;;
        --exit-after-secs)
            exit_after="$2"
            shift 2
            ;;
        --print-to-stdout)
            stdout_msg="$2"
            shift 2
            ;;
        --print-to-stderr)
            stderr_msg="$2"
            shift 2
            ;;
        --ignore-sigterm)
            ignore_sigterm=1
            shift
            ;;
        *)
            printf 'fake-mcp: unknown arg: %s\n' "$1" >&2
            exit 2
            ;;
    esac
done

if [ "$ignore_sigterm" = "1" ]; then
    # POSIX: empty handler swallows the signal.
    trap '' TERM
fi

if [ -n "$stdout_msg" ]; then
    printf '%s\n' "$stdout_msg"
fi
if [ -n "$stderr_msg" ]; then
    printf '%s\n' "$stderr_msg" >&2
fi

if [ "$exit_after" -gt 0 ]; then
    # `sleep` is POSIX. We use a small loop so SIGKILL teardown is
    # responsive — sh's `sleep N` cannot be interrupted by signals
    # other than the terminating ones, which is what we want.
    sleep "$exit_after"
fi

exit "$exit_code"
