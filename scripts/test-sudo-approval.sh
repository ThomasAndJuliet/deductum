#!/bin/sh
# Run inside a COSMIC Terminal Zellij session. The child has no controlling
# terminal and none of its standard streams is a TTY. Approval is still
# presented in the terminal window, through the sudo PAM module.
set -eu
[ -n "${ZELLIJ_SESSION_NAME:-}" ] || {
    echo 'Run this test inside the Zellij session whose approvals you are checking.' >&2
    exit 1
}
printf 'session=%s client=%s\n' "$ZELLIJ_SESSION_NAME" "${CT_CLIENT:-local}"
if output=$(setsid --wait sh -c '
    set -eu
    test ! -t 0 && test ! -t 1 && test ! -t 2
    sudo -k
    sudo true </dev/null
' </dev/null 2>&1); then
    echo 'PASS: sudo true succeeded without a TTY after terminal approval.'
else
    result=$?
    printf '%s\n' "$output" >&2
    echo "FAIL: sudo exited with $result." >&2
    exit "$result"
fi
