//! State-directory permission policy.

use std::path::Path;

#[cfg(windows)]
pub fn set_owner_only(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows_sys::Win32::Security::Authorization::SE_FILE_OBJECT;
    use windows_sys::Win32::Security::{
        GetSecurityDescriptorDacl, TokenUser, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = (|| {
        let mut size = 0u32;
        unsafe {
            windows_sys::Win32::Security::GetTokenInformation(
                token,
                TokenUser,
                std::ptr::null_mut(),
                0,
                &mut size,
            );
        }
        if size == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut buffer = vec![0u8; size as usize];
        if unsafe {
            windows_sys::Win32::Security::GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
        let mut sid_ptr = std::ptr::null_mut();
        if unsafe {
            windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW(
                user.User.Sid,
                &mut sid_ptr,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let sid = unsafe {
            let mut n = 0;
            while *sid_ptr.add(n) != 0 {
                n += 1;
            }
            let value = String::from_utf16_lossy(std::slice::from_raw_parts(sid_ptr, n));
            LocalFree(sid_ptr.cast());
            value
        };
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;{sid})\0")
            .encode_utf16()
            .collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let mut size = 0;
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                &mut size,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut present = 0;
        let mut dacl = std::ptr::null_mut();
        let mut defaulted = 0;
        let result = unsafe {
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
        };
        if result == 0 || present == 0 {
            unsafe {
                windows_sys::Win32::Foundation::LocalFree(descriptor.cast());
            }
            return Err(std::io::Error::last_os_error());
        }
        let result = unsafe {
            windows_sys::Win32::Security::Authorization::SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION
                    | windows_sys::Win32::Security::PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null(),
            )
        };
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(descriptor.cast());
        }
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(result as i32))
        }
    })();
    unsafe {
        CloseHandle(token);
    }
    result
}

#[cfg(not(windows))]
pub fn set_owner_only(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

pub fn is_owner_only(path: &Path) -> std::io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return Ok(std::fs::metadata(path)?.permissions().mode() & 0o777 == 0o700);
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
            SE_FILE_OBJECT,
        };
        use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};

        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let error = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if error != 0 {
            return Err(std::io::Error::from_raw_os_error(error as i32));
        }
        if descriptor.is_null() {
            return Err(std::io::Error::other(
                "Windows returned no security descriptor",
            ));
        }

        let mut sddl: windows_sys::core::PWSTR = std::ptr::null_mut();
        let converted = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                1,
                DACL_SECURITY_INFORMATION,
                &mut sddl,
                std::ptr::null_mut(),
            )
        };
        let result = if converted == 0 || sddl.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            let mut length = 0usize;
            while unsafe { *sddl.add(length) } != 0 {
                length += 1;
            }
            let text = unsafe { std::slice::from_raw_parts(sddl, length) };
            let sddl = String::from_utf16_lossy(text);
            let is_root = path.is_dir();
            Ok(if is_root {
                sddl_is_owner_only(&sddl)
            } else {
                sddl_is_owner_only_child(&sddl)
            })
        };
        unsafe {
            if !sddl.is_null() {
                windows_sys::Win32::Foundation::LocalFree(sddl.cast());
            }
            windows_sys::Win32::Foundation::LocalFree(descriptor.cast());
        }
        result
    }
}

/// Recognize the protected ACL emitted for a state directory. The parser is deliberately
/// independent of Windows pointers so malformed or future SDDL can only produce `false`.
pub fn sddl_is_owner_only(sddl: &str) -> bool {
    sddl_is_owner_only_with_mode(sddl, false)
}

/// Recognize the inherited ACL that a child of a protected state directory receives.
pub fn sddl_is_owner_only_child(sddl: &str) -> bool {
    sddl_is_owner_only_with_mode(sddl, true)
}

fn sddl_is_owner_only_with_mode(sddl: &str, child: bool) -> bool {
    let Some(dacl) = sddl.strip_prefix("D:") else {
        return false;
    };
    let protected = dacl.starts_with('P');
    let aces: Vec<Vec<&str>> = dacl
        .split('(')
        .skip(1)
        .filter_map(|ace| ace.strip_suffix(')').map(|ace| ace.split(';').collect()))
        .collect();
    let principals: Vec<_> = aces.iter().filter_map(|ace| ace.get(5).copied()).collect();
    (!child && protected && aces.len() == 2 || child && !aces.is_empty())
        && aces.iter().all(|ace| {
            ace.get(0) == Some(&"A")
                && ace.get(2) == Some(&"FA")
                && (child || ace.get(1) == Some(&"OICI"))
        })
        && principals.contains(&"SY")
        && principals
            .iter()
            .any(|principal| principal.starts_with("S-1-5-21-"))
        && principals
            .iter()
            .all(|principal| *principal == "SY" || principal.starts_with("S-1-5-21-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_state_dir_is_owner_only() {
        // The Windows test guard's profile temp parent is intentionally not granted
        // WRITE_DAC; use Cargo's isolated workspace target instead.
        let target = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..\\..\\target");
        let parent = tempfile::Builder::new().tempdir_in(target).unwrap();
        let dir = parent.path().join("state");
        crate::store::ensure_state_root(&dir).unwrap();
        assert!(is_owner_only(&dir).unwrap());
    }

    #[test]
    fn files_created_inside_inherit_owner_only() {
        let target = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..\\..\\target");
        let parent = tempfile::Builder::new().tempdir_in(target).unwrap();
        let dir = parent.path().join("state");
        crate::store::ensure_state_root(&dir).unwrap();
        let file = dir.join("child");
        std::fs::write(&file, b"fixture").unwrap();
        assert!(is_owner_only(&file).unwrap());
    }

    #[test]
    fn sddl_is_owner_only_rules() {
        for (sddl, expected, child) in [
            (
                "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;S-1-5-21-123-456-789-1001)",
                true,
                false,
            ),
            ("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;WD)", false, false),
            (
                "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;S-1-5-21-123-456-789-1001)(A;OICI;FA;;;AU)",
                false,
                false,
            ),
            (
                "D:AI(A;OICI;FA;;;SY)(A;OICI;FA;;;S-1-5-21-123-456-789-1001)",
                false,
                false,
            ),
            (
                "D:AI(A;ID;FA;;;SY)(A;ID;FA;;;S-1-5-21-123-456-789-1001)",
                true,
                true,
            ),
        ] {
            let actual = if child {
                sddl_is_owner_only_child(sddl)
            } else {
                sddl_is_owner_only(sddl)
            };
            assert_eq!(actual, expected, "{sddl}");
        }
        assert!(sddl_is_owner_only_child(
            "D:AI(A;ID;FA;;;SY)(A;ID;FA;;;S-1-5-21-123-456-789-1001)"
        ));
        assert!(!sddl_is_owner_only(
            "D:AI(A;ID;FA;;;SY)(A;ID;FA;;;S-1-5-21-123-456-789-1001)"
        ));
    }
}
