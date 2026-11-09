//! Local OS services. Remote paths always belong to their session host.

use std::path::PathBuf;

pub fn canonicalize(path: &std::path::Path) -> std::io::Result<PathBuf> {
    let path = std::fs::canonicalize(path)?;
    // Rust returns extended-length paths on Windows. Keep ordinary drive/UNC
    // spelling at the JSON boundary so prover file URIs retain their identity.
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(tail) = text.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{tail}")));
        }
        if let Some(tail) = text.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(tail));
        }
    }
    Ok(path)
}

pub fn home() -> PathBuf {
    #[cfg(windows)]
    let variable = "USERPROFILE";
    #[cfg(not(windows))]
    let variable = "HOME";
    std::env::var_os(variable).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// The parent of application configuration folders, not Deductum's folder.
pub fn config_home() -> PathBuf {
    #[cfg(windows)]
    let variable = "APPDATA";
    #[cfg(not(windows))]
    let variable = "XDG_CONFIG_HOME";
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("DEDUCTUM_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| config_home().join("deductum"))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// The `PATH` an app bundle should run with: its bundled Python, then the places a Mac keeps
/// the Lean toolchain and Homebrew tools, ahead of whatever `PATH` already holds. Finder gives
/// apps no shell `PATH`. Entries already present are not repeated; the others keep their order.
pub fn bundle_path(
    resources: &std::path::Path,
    arm64: bool,
    home: &std::path::Path,
    existing: Option<&std::ffi::OsStr>,
) -> Option<std::ffi::OsString> {
    let python = resources.join("python").join(if arm64 { "arm64" } else { "x86_64" }).join("bin");
    let wanted = [python, home.join(".elan/bin"), PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")];
    let existing: Vec<PathBuf> = existing.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    let mut entries: Vec<PathBuf> = Vec::new();
    for entry in wanted {
        if !existing.contains(&entry) && !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    entries.extend(existing);
    std::env::join_paths(entries).ok()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// The `Contents/Resources` folder of the `.app` bundle that `exe` runs from, if it does
pub fn bundle_resources(exe: &std::path::Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let inside = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension().is_some_and(|extension| extension == "app");
    inside.then(|| contents.join("Resources"))
}

/// Gives an app opened from Finder the `PATH` it needs. Call it first in `main`, before any
/// thread starts: changing the environment is not safe once there are other threads.
#[cfg(target_os = "macos")]
pub fn prepare_bundle_environment() {
    let Some(resources) = std::env::current_exe().ok().and_then(|exe| bundle_resources(&exe)) else { return };
    let existing = std::env::var_os("PATH");
    if let Some(path) = bundle_path(&resources, cfg!(target_arch = "aarch64"), &home(), existing.as_deref()) {
        // SAFETY: called at the start of main, while the process is single-threaded
        unsafe { std::env::set_var("PATH", path) };
    }
}

// The bundle path is built only on macOS; the tests use Unix paths and `:`, so they run on Unix
#[cfg(all(test, unix))]
mod bundle_tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;

    fn paths(value: Option<OsString>) -> Vec<PathBuf> {
        std::env::split_paths(&value.unwrap()).collect()
    }

    #[test]
    fn the_bundled_python_comes_first_for_each_architecture() {
        let resources = Path::new("/Applications/Deductum.app/Contents/Resources");
        let home = Path::new("/Users/me");
        let existing = OsString::from("/usr/bin:/bin");
        let arm = paths(bundle_path(resources, true, home, Some(&existing)));
        assert_eq!(arm, [
            "/Applications/Deductum.app/Contents/Resources/python/arm64/bin", "/Users/me/.elan/bin",
            "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin",
        ].map(PathBuf::from));
        let intel = paths(bundle_path(resources, false, home, Some(&existing)));
        assert_eq!(intel[0], Path::new("/Applications/Deductum.app/Contents/Resources/python/x86_64/bin"));
    }

    #[test]
    fn entries_already_on_the_path_are_not_repeated_or_moved() {
        let existing = OsString::from("/usr/bin:/opt/homebrew/bin:/bin");
        let got = paths(bundle_path(Path::new("/A.app/Contents/Resources"), true, Path::new("/h"), Some(&existing)));
        assert_eq!(got, ["/A.app/Contents/Resources/python/arm64/bin", "/h/.elan/bin", "/usr/local/bin",
            "/usr/bin", "/opt/homebrew/bin", "/bin"].map(PathBuf::from));
    }

    #[test]
    fn a_missing_path_still_gets_the_entries() {
        let got = paths(bundle_path(Path::new("/A.app/Contents/Resources"), false, Path::new("/h"), None));
        assert_eq!(got.len(), 4);
    }

    #[test]
    fn only_an_executable_inside_an_app_bundle_has_resources() {
        assert_eq!(bundle_resources(Path::new("/Applications/Deductum.app/Contents/MacOS/deductum")),
            Some(PathBuf::from("/Applications/Deductum.app/Contents/Resources")));
        assert_eq!(bundle_resources(Path::new("/x/target/release/deductum")), None);
        assert_eq!(bundle_resources(Path::new("/x/Deductum/Contents/MacOS/deductum")), None);
        assert_eq!(bundle_resources(Path::new("/x/Deductum.app/MacOS/deductum")), None);
        assert_eq!(bundle_resources(Path::new("deductum")), None);
    }
}

#[cfg(windows)]
pub mod windows {
    use std::ffi::{OsStr, c_void};
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain(Some(0)).collect()
    }

    pub fn current_sid() -> io::Result<String> {
        token_sid(unsafe { GetCurrentProcess() })
    }

    fn token_sid(process: HANDLE) -> io::Result<String> {
        // Windows token structures contain pointers. Use usize-aligned storage and
        // keep it alive until the SID has been converted to an owned string.
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut length = 0;
            GetTokenInformation(token, TokenUser, null_mut(), 0, &mut length);
            let mut storage = vec![0_usize; (length as usize).div_ceil(size_of::<usize>())];
            let ok = GetTokenInformation(token, TokenUser, storage.as_mut_ptr().cast(), length, &mut length);
            let error = io::Error::last_os_error();
            CloseHandle(token);
            if ok == 0 {
                return Err(error);
            }
            let user = &*storage.as_ptr().cast::<TOKEN_USER>();
            let mut text = null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut count = 0;
            while *text.add(count) != 0 {
                count += 1;
            }
            let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, count));
            LocalFree(text.cast());
            Ok(result)
        }
    }

    pub fn pipe_has_owner(handle: HANDLE, sid: &str) -> io::Result<bool> {
        // Pipe names live in a shared namespace: authenticating only the name
        // would disclose launch requests to another user's pre-created server.
        unsafe {
            let mut pid = 0;
            if windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId(handle, &mut pid) == 0 {
                return Err(io::Error::last_os_error());
            }
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return Err(io::Error::last_os_error());
            }
            let owner = token_sid(process);
            CloseHandle(process);
            owner.map(|owner| owner == sid)
        }
    }

    fn console_arguments(arguments: &[String]) -> String {
        arguments.iter().map(|argument| {
            // CRT argument quoting, including trailing slashes and literal quotes.
            // ShellExecute receives a command line, never a cmd/PowerShell script.
            let mut quoted = String::from("\"");
            let mut slashes = 0;
            for character in argument.chars() {
                if character == '\\' {
                    slashes += 1;
                } else {
                    quoted.extend(std::iter::repeat_n('\\', slashes * if character == '"' { 2 } else { 1 }));
                    if character == '"' { quoted.push('\\'); }
                    quoted.push(character);
                    slashes = 0;
                }
            }
            quoted.extend(std::iter::repeat_n('\\', slashes * 2));
            quoted.push('"');
            quoted
        }).collect::<Vec<_>>().join(" ")
    }

    pub fn launch_console(program: &str, arguments: &[String], directory: Option<&str>) -> io::Result<()> {
        use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        if program.contains('\0') || arguments.iter().any(|value| value.contains('\0')) || directory.is_some_and(|value| value.contains('\0')) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a console argument contains NUL"));
        }
        let program = wide(program);
        let arguments = wide(&console_arguments(arguments));
        let directory = directory.map(wide);
        unsafe {
            let initialized = CoInitializeEx(null_mut(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32);
            if initialized < 0 {
                return Err(io::Error::other(format!("could not initialize the console launcher: {initialized:#x}")));
            }
            // The shell creates fresh console handles. CreateProcess inheriting
            // this GUI's absent standard handles produces a noninteractive shell.
            let result = ShellExecuteW(null_mut(), wide("open").as_ptr(), program.as_ptr(), arguments.as_ptr(),
                directory.as_ref().map_or(std::ptr::null(), |value| value.as_ptr()), SW_SHOWNORMAL) as isize;
            CoUninitialize();
            if result <= 32 {
                Err(io::Error::other(format!("Windows could not open the console (shell error {result})")))
            } else {
                Ok(())
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn console_arguments_preserve_unicode_quotes_spaces_and_slashes() {
            let expected = vec!["", "-t", "user@proof-server", "C:\\Å proof 😀\\", "quote\"and\\\"slashes", "cd '/srv/a b' && exec \"$SHELL\" -l"];
            let arguments = expected.iter().map(|value| value.to_string()).collect::<Vec<_>>();
            let command = wide(&format!("console.exe {}", console_arguments(&arguments)));
            unsafe {
                let mut count = 0;
                let argv = windows_sys::Win32::UI::Shell::CommandLineToArgvW(command.as_ptr(), &mut count);
                assert!(!argv.is_null());
                let actual = std::slice::from_raw_parts(argv, count as usize).iter().skip(1).map(|value| {
                    let mut length = 0;
                    while *value.add(length) != 0 { length += 1; }
                    String::from_utf16_lossy(std::slice::from_raw_parts(*value, length))
                }).collect::<Vec<_>>();
                LocalFree(argv.cast());
                assert_eq!(actual, expected);
            }
        }
    }

    /// Owns the descriptor referenced by a named pipe's security attributes.
    pub struct PipeSecurity(*mut c_void);

    impl PipeSecurity {
        pub fn for_user(sid: &str) -> io::Result<Self> {
            let sddl = wide(&format!("D:P(A;;GA;;;{sid})(A;;GA;;;SY)"));
            let mut descriptor = null_mut();
            unsafe {
                if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, null_mut()) == 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(Self(descriptor))
        }

        pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
            SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.0,
                bInheritHandle: 0,
            }
        }
    }

    impl Drop for PipeSecurity {
        fn drop(&mut self) {
            unsafe { LocalFree(self.0); }
        }
    }
}
