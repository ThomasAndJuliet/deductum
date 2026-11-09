Deductum for macOS

Deductum.app runs on Apple Silicon and Intel Macs with macOS 11 or later.
It includes its own Python, so no system Python is needed. To use Lean,
install elan first: https://github.com/leanprover/elan

Install: drag Deductum.app to Applications (or to ~/Applications).

The app is not signed with an Apple Developer ID, so macOS blocks it after
a download. Open Terminal and run:

  xattr -dr com.apple.quarantine /Applications/Deductum.app

(use ~/Applications/Deductum.app if you put it there). Then open Deductum
like any other app.

In the app, choose New session, pick the project folder, choose Lean or
Isabelle and then Start session. Closing a window leaves proof sessions
running. Use Stop in the application to stop a particular prover.

To open a project straight away, run the app from Terminal with the folder:

  /Applications/Deductum.app/Contents/MacOS/deductum ~/path/to/project

To update, quit Deductum and replace the app with the new one. Settings and
sessions are kept.

Settings are in ~/.config/deductum, sessions and recovery data are in
~/.local/state/deductum, and the session host is installed in
~/.local/share/deductum. Deleting the app does not delete them.

RELEASE.json under Deductum.app/Contents/Resources/share/deductum records
the source commit, development status, binary and host hashes and the
Python runtime.
