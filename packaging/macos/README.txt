Deductum for macOS

This folder runs on Apple Silicon and Intel Macs with macOS 11 or later.
It includes its own Python, so no system Python is needed. To use Lean,
install elan first: https://github.com/leanprover/elan

The package is not signed with an Apple Developer ID, so macOS blocks it
after a download. Open Terminal, type the line below with a space at the
end, drag this folder into the Terminal window and press Return:

  xattr -dr com.apple.quarantine

Then double-click Deductum.command. To open a project straight away, run it
from Terminal with the project folder:

  ./Deductum.command ~/path/to/lake-project

In the app, choose New session, pick the project folder, choose Lean and
then Start session. Closing a window leaves proof sessions running. Use Stop
in the application to stop a particular prover.

On the first start the app adds a "deductum" MCP server to ~/.claude.json
(and to ~/.codex/config.toml if Codex is used) that runs python3. On a Mac
that is Python 3.9 or missing, so set its "command" to the full path of
python/arm64/bin/python3 (Apple Silicon) or python/x86_64/bin/python3
(Intel) in this folder.

Settings are in ~/.config/deductum, sessions and recovery data are in
~/.local/state/deductum, and the session host is installed in
~/.local/share/deductum. Deleting this folder does not delete them.

RELEASE.json under share/deductum records the source commit, development
status, binary and host hashes and the Python runtime.
