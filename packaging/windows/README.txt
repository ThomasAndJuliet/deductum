Deductum for Windows x64

Extract this complete archive into a new folder in your user profile, for
example %LOCALAPPDATA%\Programs\Deductum\VERSION. Run deductum.exe. Keep the
python and share folders beside it. The package includes the Python runtime;
it does not need a system Python installation or WSL.

Local proofs need native Lean (elan) or the official Isabelle2025 Windows
distribution, including its bundled Cygwin. Install those separately.
Its Java command-line tools need existing Windows short-path aliases for
some Unicode paths. If the volume has no usable alias, Deductum identifies
the path that must use an ASCII location for that Isabelle operation.
Local Isabelle logic names and command-line option values must use ASCII.
Remote Linux connections use Windows OpenSSH and tar. Your SSH key and host
trust are configured with the normal Windows SSH client, outside Deductum.
Add machines to %APPDATA%\deductum\machines.json as a JSON array, for example:
[{"name":"proof-server","host":"proof-server","user":"my-user"}]
You can also run deductum.exe --machine my-user@proof-server /remote/project.
Remote project paths belong to the server; local paths belong to Windows.

Settings are in %APPDATA%\deductum; sessions and recovery data are in
%LOCALAPPDATA%\deductum\state. Closing a window leaves proof sessions running.
Use Stop in the application to stop a particular prover.

To update, extract the new archive to a DIFFERENT folder. Close the app
windows, run stop-host.cmd to flush and stop the old session host, then run
deductum.exe from the new folder. A running host from another package is
reported instead of silently replaced. Do not overwrite an active package.
Keep the previous package until you have verified the new one. User data is
kept outside package folders; deleting an old package does not delete it.

This is a private household build. RELEASE.json under share\deductum records
its source commit, development status, binary/host hashes and Python runtime.
Development packages are explicitly marked and are not release evidence.
