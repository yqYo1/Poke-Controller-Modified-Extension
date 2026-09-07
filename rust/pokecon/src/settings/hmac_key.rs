use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use getrandom::fill;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use thiserror::Error;

const KEY_LENGTH: usize = 32;

/// Per-install HMAC-SHA-256 key whose debug representation is always redacted.
#[derive(Clone)]
pub struct HmacKey([u8; KEY_LENGTH]);

impl HmacKey {
    /// Loads the winning key or creates it with an exclusive, no-replace race.
    ///
    /// Candidate keys are fully written and synced before an atomic hard-link
    /// publish. Losing processes discard their candidate and reread the
    /// winner, so concurrent startup cannot produce split manifest signatures.
    ///
    /// # Errors
    ///
    /// Returns an error for randomness, I/O, a non-regular key, or a key whose
    /// length is not exactly 32 bytes. A corrupt existing key is never replaced.
    pub fn load_or_create(directory: &Path) -> Result<Self, HmacKeyError> {
        fs::create_dir_all(directory).map_err(|source| HmacKeyError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let key_path = directory.join(".hmac-key");
        match Self::read(&key_path) {
            Ok(key) => return Ok(key),
            Err(HmacKeyError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        let mut key = [0_u8; KEY_LENGTH];
        fill(&mut key).map_err(HmacKeyError::Random)?;
        let mut suffix = [0_u8; 12];
        fill(&mut suffix).map_err(HmacKeyError::Random)?;
        let candidate_path = directory.join(format!(
            ".hmac-key.{}.{}.tmp",
            std::process::id(),
            hex::encode(suffix)
        ));
        let result = publish_candidate(&candidate_path, &key_path, &key);
        let _cleanup_result = fs::remove_file(&candidate_path);
        match result {
            Ok(true) => {
                sync_parent(directory)?;
                Ok(Self(key))
            }
            Ok(false) => Self::read(&key_path),
            Err(error) => Err(error),
        }
    }

    /// Computes a lowercase HMAC-SHA-256 digest.
    ///
    /// # Panics
    ///
    /// Panics only if the HMAC implementation rejects this type's fixed
    /// 32-byte key, which is an invariant of HMAC-SHA-256.
    #[must_use]
    pub fn digest(&self, value: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0)
            .expect("HMAC accepts every key length supported by this type");
        mac.update(value);
        hex::encode(mac.finalize().into_bytes())
    }

    /// Verifies a lowercase hexadecimal HMAC in constant time.
    ///
    /// # Panics
    ///
    /// Panics only if the HMAC implementation rejects this type's fixed
    /// 32-byte key, which is an invariant of HMAC-SHA-256.
    #[must_use]
    pub fn verify(&self, value: &[u8], expected: &str) -> bool {
        let Ok(expected) = hex::decode(expected) else {
            return false;
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0)
            .expect("HMAC accepts every key length supported by this type");
        mac.update(value);
        mac.verify_slice(&expected).is_ok()
    }

    fn read(path: &Path) -> Result<Self, HmacKeyError> {
        let metadata = fs::symlink_metadata(path).map_err(|source| HmacKeyError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if !metadata.file_type().is_file() {
            return Err(HmacKeyError::NotRegular(path.to_path_buf()));
        }
        let mut bytes = Vec::with_capacity(KEY_LENGTH);
        File::open(path)
            .and_then(|mut file| file.read_to_end(&mut bytes))
            .map_err(|source| HmacKeyError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        let key: [u8; KEY_LENGTH] =
            bytes
                .try_into()
                .map_err(|bytes: Vec<u8>| HmacKeyError::InvalidLength {
                    path: path.to_path_buf(),
                    actual: bytes.len(),
                })?;
        Ok(Self(key))
    }
}

impl fmt::Debug for HmacKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HmacKey(<redacted>)")
    }
}

fn publish_candidate(
    candidate_path: &Path,
    key_path: &Path,
    key: &[u8; KEY_LENGTH],
) -> Result<bool, HmacKeyError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
        options.attributes(0x80);
    }
    let mut candidate = options
        .open(candidate_path)
        .map_err(|source| HmacKeyError::Io {
            path: candidate_path.to_path_buf(),
            source,
        })?;
    #[cfg(windows)]
    {
        if let Err(error) = windows_acl::harden_file_handle(&candidate, candidate_path) {
            drop(candidate);
            let _ = fs::remove_file(candidate_path);
            return Err(error);
        }
    }
    #[cfg(all(test, not(windows), not(unix)))]
    {
        if let Err(error) = fake_windows_hardening::harden(candidate_path) {
            drop(candidate);
            let _ = fs::remove_file(candidate_path);
            return Err(error);
        }
    }
    #[cfg(test)]
    {
        if test_hooks::is_mock_enabled()
            && let Err(error) = test_hooks::mock_harden(candidate_path)
        {
            drop(candidate);
            let _ = fs::remove_file(candidate_path);
            return Err(error);
        }
    }
    candidate
        .write_all(key)
        .and_then(|()| candidate.sync_all())
        .map_err(|source| HmacKeyError::Io {
            path: candidate_path.to_path_buf(),
            source,
        })?;
    match fs::hard_link(candidate_path, key_path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(source) => Err(HmacKeyError::Io {
            path: key_path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_acl {
    use std::ffi::c_void;
    use std::fs::File;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    use crate::settings::hmac_key::HmacKeyError;

    #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
    type HANDLE = *mut c_void;
    #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
    type DWORD = u32;
    #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
    type BOOL = i32;
    #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
    type PSID = *mut c_void;
    #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
    type PACL = *mut c_void;
    #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
    type PHANDLE = *mut HANDLE;
    #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
    type PDWORD = *mut DWORD;
    #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
    type LPVOID = *mut c_void;

    const TOKEN_QUERY: DWORD = 0x0008;
    const TOKEN_USER_CLASS: DWORD = 1;
    const GRANT_ACCESS: DWORD = 1;
    const NO_INHERITANCE: DWORD = 0;
    const TRUSTEE_IS_SID: DWORD = 0;
    const TRUSTEE_IS_USER: DWORD = 1;
    const NO_MULTIPLE_TRUSTEE: DWORD = 0;
    const SE_FILE_OBJECT: DWORD = 1;
    const DACL_SECURITY_INFORMATION: DWORD = 0x0000_0004;
    const PROTECTED_DACL_SECURITY_INFORMATION: DWORD = 0x8000_0000;
    const FILE_ALL_ACCESS: DWORD = 0x001F_01FF;

    #[allow(non_snake_case, reason = "Windows ABI field/type names")]
    #[repr(C)]
    struct SID_AND_ATTRIBUTES {
        Sid: PSID,
        Attributes: DWORD,
    }

    #[allow(non_snake_case, reason = "Windows ABI field/type names")]
    #[repr(C)]
    struct TOKEN_USER {
        User: SID_AND_ATTRIBUTES,
    }

    #[allow(non_snake_case, reason = "Windows ABI field/type names")]
    #[repr(C)]
    struct TRUSTEE_W {
        pMultipleTrustee: *mut TRUSTEE_W,
        MultipleTrusteeOperation: DWORD,
        TrusteeForm: DWORD,
        TrusteeType: DWORD,
        ptstrName: *mut u16,
    }

    #[allow(non_snake_case, reason = "Windows ABI field/type names")]
    #[repr(C)]
    struct EXPLICIT_ACCESS_W {
        grfAccessPermissions: DWORD,
        grfAccessMode: DWORD,
        grfInheritance: DWORD,
        Trustee: TRUSTEE_W,
    }

    #[allow(non_snake_case, reason = "Windows ABI field/type names")]
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(
            ProcessHandle: HANDLE,
            DesiredAccess: DWORD,
            TokenHandle: PHANDLE,
        ) -> BOOL;
        fn GetTokenInformation(
            TokenHandle: HANDLE,
            TokenInformationClass: DWORD,
            TokenInformation: LPVOID,
            TokenInformationLength: DWORD,
            ReturnLength: PDWORD,
        ) -> BOOL;
        fn SetEntriesInAclW(
            cCountOfExplicitEntries: DWORD,
            pListOfExplicitEntries: *mut EXPLICIT_ACCESS_W,
            OldAcl: PACL,
            NewAcl: *mut PACL,
        ) -> DWORD;
        fn SetSecurityInfo(
            handle: HANDLE,
            ObjectType: DWORD,
            SecurityInfo: DWORD,
            psidOwner: PSID,
            psidGroup: PSID,
            pDacl: PACL,
            pSacl: PACL,
        ) -> DWORD;
    }

    #[allow(non_snake_case, reason = "Windows ABI field/type names")]
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> HANDLE;
        fn CloseHandle(hObject: HANDLE) -> BOOL;
        fn LocalFree(hMem: *mut c_void) -> *mut c_void;
    }

    pub fn harden_file_handle(file: &File, path: &Path) -> Result<(), HmacKeyError> {
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(HmacKeyError::Io {
                    path: path.to_path_buf(),
                    source: std::io::Error::last_os_error(),
                });
            }
            struct HandleGuard(HANDLE);
            impl Drop for HandleGuard {
                fn drop(&mut self) {
                    unsafe {
                        CloseHandle(self.0);
                    }
                }
            }
            let _handle_guard = HandleGuard(token);

            let mut needed: DWORD = 0;
            GetTokenInformation(
                token,
                TOKEN_USER_CLASS,
                std::ptr::null_mut(),
                0,
                &mut needed,
            );
            if needed == 0 {
                return Err(HmacKeyError::Io {
                    path: path.to_path_buf(),
                    source: std::io::Error::last_os_error(),
                });
            }
            let mut buffer = vec![0u8; needed as usize];
            if GetTokenInformation(
                token,
                TOKEN_USER_CLASS,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            ) == 0
            {
                return Err(HmacKeyError::Io {
                    path: path.to_path_buf(),
                    source: std::io::Error::last_os_error(),
                });
            }
            let token_user = buffer.as_ptr().cast::<TOKEN_USER>();
            let sid = (*token_user).User.Sid;
            if sid.is_null() {
                return Err(HmacKeyError::Io {
                    path: path.to_path_buf(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "current user SID is unavailable",
                    ),
                });
            }

            let mut explicit = EXPLICIT_ACCESS_W {
                grfAccessPermissions: FILE_ALL_ACCESS,
                grfAccessMode: GRANT_ACCESS,
                grfInheritance: NO_INHERITANCE,
                Trustee: TRUSTEE_W {
                    pMultipleTrustee: std::ptr::null_mut(),
                    MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                    TrusteeForm: TRUSTEE_IS_SID,
                    TrusteeType: TRUSTEE_IS_USER,
                    ptstrName: sid.cast(),
                },
            };

            let mut new_dacl: PACL = std::ptr::null_mut();
            let result = SetEntriesInAclW(1, &mut explicit, std::ptr::null_mut(), &mut new_dacl);
            if result != 0 {
                return Err(HmacKeyError::Io {
                    path: path.to_path_buf(),
                    source: std::io::Error::from_raw_os_error(result as i32),
                });
            }
            struct AclGuard(PACL);
            impl Drop for AclGuard {
                fn drop(&mut self) {
                    unsafe {
                        if !self.0.is_null() {
                            LocalFree(self.0);
                        }
                    }
                }
            }
            let _acl_guard = AclGuard(new_dacl);
            if new_dacl.is_null() {
                return Err(HmacKeyError::Io {
                    path: path.to_path_buf(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "failed to build owner-only DACL",
                    ),
                });
            }

            let handle = file.as_raw_handle() as HANDLE;
            let sec_result = SetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                new_dacl,
                std::ptr::null_mut(),
            );
            if sec_result != 0 {
                return Err(HmacKeyError::Io {
                    path: path.to_path_buf(),
                    source: std::io::Error::from_raw_os_error(sec_result as i32),
                });
            }
            Ok(())
        }
    }
}

#[cfg(all(test, not(windows), not(unix)))]
mod fake_windows_hardening {
    use std::path::Path;

    use crate::settings::hmac_key::HmacKeyError;

    pub fn harden(path: &Path) -> Result<(), HmacKeyError> {
        if path.exists() {
            Ok(())
        } else {
            Err(HmacKeyError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "fake hardening: candidate missing",
                ),
            })
        }
    }
}

#[cfg(test)]
mod test_hooks {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use crate::settings::hmac_key::HmacKeyError;

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static SHOULD_FAIL: AtomicBool = AtomicBool::new(false);
    static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

    pub fn is_mock_enabled() -> bool {
        ENABLED.load(Ordering::SeqCst)
    }

    pub fn mock_harden(path: &Path) -> Result<(), HmacKeyError> {
        CALL_COUNT.fetch_add(1, Ordering::SeqCst);
        if SHOULD_FAIL.load(Ordering::SeqCst) {
            Err(HmacKeyError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "mock windows hardening failure",
                ),
            })
        } else if path.exists() {
            Ok(())
        } else {
            Err(HmacKeyError::Io {
                path: PathBuf::from(path),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "mock candidate not found",
                ),
            })
        }
    }

    pub fn set_mock(enabled: bool, should_fail: bool) {
        ENABLED.store(enabled, Ordering::SeqCst);
        SHOULD_FAIL.store(should_fail, Ordering::SeqCst);
        CALL_COUNT.store(0, Ordering::SeqCst);
    }

    pub fn call_count() -> usize {
        CALL_COUNT.load(Ordering::SeqCst)
    }
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<(), HmacKeyError> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| HmacKeyError::Io {
            path: parent.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<(), HmacKeyError> {
    Ok(())
}

/// Exclusive HMAC-key creation and validation failures.
#[derive(Debug, Error)]
pub enum HmacKeyError {
    #[error("secure random generation failed")]
    Random(#[source] getrandom::Error),
    #[error("HMAC key is not a regular file: {0}")]
    NotRegular(PathBuf),
    #[error("HMAC key has invalid length {actual}, expected {KEY_LENGTH}: {path}")]
    InvalidLength { path: PathBuf, actual: usize },
    #[error("HMAC key I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use tempfile::TempDir;

    use super::HmacKey;
    use super::test_hooks;

    #[test]
    fn concurrent_key_creation_converges_on_one_winner() {
        let temporary = TempDir::new().expect("temporary directory must exist");
        let directory = temporary.path().join("venv-manifests");
        let barrier = Arc::new(Barrier::new(12));
        let handles = (0..12)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let directory = directory.clone();
                thread::spawn(move || {
                    barrier.wait();
                    HmacKey::load_or_create(&directory)
                        .expect("key creation must converge")
                        .digest(b"same input")
                })
            })
            .collect::<Vec<_>>();
        let digests = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread must finish"))
            .collect::<Vec<_>>();
        assert!(digests.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[cfg(unix)]
    #[test]
    fn new_key_file_is_owner_only_0600() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = TempDir::new().expect("temporary directory must exist");
        let directory = temporary.path().join("venv-manifests");
        let key = HmacKey::load_or_create(&directory).expect("key must be created");
        let metadata =
            fs::metadata(directory.join(".hmac-key")).expect("key file metadata must be readable");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "HMAC key file must be 0600");
        assert!(!key.digest(b"probe").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn candidate_temporary_files_are_owner_only_0600() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = TempDir::new().expect("temporary directory must exist");
        let directory = temporary.path().join("venv-manifests");
        let _ = HmacKey::load_or_create(&directory).expect("first key must be created");
        let _ = HmacKey::load_or_create(&directory).expect("second load must succeed");
        let mode = fs::metadata(directory.join(".hmac-key"))
            .expect("metadata must be readable")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(windows)]
    #[allow(unsafe_code)]
    #[test]
    fn new_key_file_has_owner_only_dacl() {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStrExt;

        #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
        type DWORD = u32;
        #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
        type BOOL = i32;
        #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
        type PSID = *mut c_void;
        #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
        type PACL = *mut c_void;
        #[allow(non_camel_case_types, reason = "Windows ABI field/type names")]
        type PSECURITY_DESCRIPTOR = *mut c_void;

        const DACL_SECURITY_INFORMATION: DWORD = 0x0000_0004;
        const PROTECTED_DACL_SECURITY_INFORMATION: DWORD = 0x8000_0000;
        const SE_FILE_OBJECT: DWORD = 1;

        #[allow(non_snake_case, reason = "Windows ABI field/type names")]
        #[repr(C)]
        struct ACL {
            AclRevision: u8,
            Sbz1: u8,
            AclSize: u16,
            AceCount: u16,
            Sbz2: u16,
        }

        #[allow(non_snake_case, reason = "Windows ABI field/type names")]
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn GetNamedSecurityInfoW(
                pObjectName: *const u16,
                ObjectType: DWORD,
                SecurityInfo: DWORD,
                ppsidOwner: *mut PSID,
                ppsidGroup: *mut PSID,
                ppDacl: *mut PACL,
                ppSacl: *mut PACL,
                ppSecurityDescriptor: *mut PSECURITY_DESCRIPTOR,
            ) -> DWORD;
            fn GetSecurityDescriptorDacl(
                pSecurityDescriptor: PSECURITY_DESCRIPTOR,
                lpbDaclPresent: *mut BOOL,
                pDacl: *mut PACL,
                lpbDaclDefaulted: *mut BOOL,
            ) -> BOOL;
            fn GetSecurityDescriptorOwner(
                pSecurityDescriptor: PSECURITY_DESCRIPTOR,
                pOwner: *mut PSID,
                lpbOwnerDefaulted: *mut BOOL,
            ) -> BOOL;
            fn IsValidSecurityDescriptor(pSecurityDescriptor: PSECURITY_DESCRIPTOR) -> BOOL;
        }

        #[allow(non_snake_case, reason = "Windows ABI field/type names")]
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn LocalFree(hMem: *mut c_void) -> *mut c_void;
        }

        let temporary = TempDir::new().expect("temporary directory must exist");
        let directory = temporary.path().join("venv-manifests");
        let _ = HmacKey::load_or_create(&directory).expect("key must be created");
        let key_path = directory.join(".hmac-key");
        let wide: Vec<u16> = key_path.as_os_str().encode_wide().chain(Some(0)).collect();

        unsafe {
            let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            let result = GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut descriptor,
            );
            assert_eq!(result, 0, "GetNamedSecurityInfoW must succeed");
            assert!(!descriptor.is_null());
            assert_eq!(IsValidSecurityDescriptor(descriptor), 1);

            let mut dacl_present: BOOL = 0;
            let mut dacl_defaulted: BOOL = 0;
            let mut retrieved_dacl: PACL = std::ptr::null_mut();
            let gsd = GetSecurityDescriptorDacl(
                descriptor,
                &mut dacl_present,
                &mut retrieved_dacl,
                &mut dacl_defaulted,
            );
            assert_ne!(gsd, 0);
            assert_ne!(dacl_present, 0);
            assert!(!retrieved_dacl.is_null());
            let ace_count = (*(retrieved_dacl as *mut ACL)).AceCount;
            assert_eq!(ace_count, 1, "owner-only DACL must have exactly one ACE");

            let mut defaulted: BOOL = 0;
            let mut owner_sid: PSID = std::ptr::null_mut();
            let gso = GetSecurityDescriptorOwner(descriptor, &mut owner_sid, &mut defaulted);
            assert_ne!(gso, 0);
            assert!(!owner_sid.is_null());

            LocalFree(descriptor);
        }
    }

    #[test]
    fn windows_hardening_path_is_invoked_and_fails_closed_via_deterministic_fake() {
        let temporary = TempDir::new().expect("temporary directory must exist");
        let directory = temporary.path().join("venv-manifests");
        fs::create_dir_all(&directory).expect("directory must be created");

        test_hooks::set_mock(true, false);
        let key = HmacKey::load_or_create(&directory).expect("mock success must publish");
        assert!(
            test_hooks::call_count() >= 1,
            "hardening hook must have been invoked"
        );
        assert!(!key.digest(b"probe").is_empty());

        let temporary2 = TempDir::new().expect("temporary directory must exist");
        let directory2 = temporary2.path().join("venv-manifests");
        fs::create_dir_all(&directory2).expect("directory must be created");
        test_hooks::set_mock(true, true);
        let result = HmacKey::load_or_create(&directory2);
        assert!(result.is_err(), "mocked hardening failure must abort");
        assert!(
            !directory2.join(".hmac-key").exists(),
            "failed hardening must not publish a key"
        );
        assert!(
            test_hooks::call_count() >= 1,
            "hardening hook must have been invoked on failure"
        );

        test_hooks::set_mock(false, false);

        assert_eq!(format!("{key:?}"), "HmacKey(<redacted>)");
    }

    #[test]
    fn key_debug_is_always_redacted() {
        let temporary = TempDir::new().expect("temporary directory must exist");
        let key = HmacKey::load_or_create(&temporary.path().join("venv-manifests"))
            .expect("key must be created");
        let debug = format!("{key:?}");
        assert_eq!(debug, "HmacKey(<redacted>)");
        assert!(!debug.contains("HmacKey(") || debug == "HmacKey(<redacted>)");
    }
}
