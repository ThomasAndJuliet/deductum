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
