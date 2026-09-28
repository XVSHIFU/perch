use crate::store::{Failure, Result};

pub trait Secrets {
    fn put(&self, reference: &str, value: &str) -> Result<()>;
    fn delete(&self, reference: &str) -> Result<()>;
    fn get(&self, _reference: &str) -> Result<String> {
        Err(Failure::new("CREDENTIAL_READ", "无法读取密钥，请重新填写"))
    }
}
pub struct SystemSecrets;

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn windows_credential_roundtrip_isolated_test_entry() {
        use windows_sys::Win32::Security::Credentials::*;
        let reference = format!("test-{}", uuid::Uuid::new_v4());
        let vault = SystemSecrets;
        vault.put(&reference, "perch-test-no-real-key").unwrap();
        let target: Vec<u16> = format!("Perch/model/{reference}\0")
            .encode_utf16()
            .collect();
        let mut ptr = std::ptr::null_mut();
        let success = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut ptr) } != 0;
        let matches = if success {
            unsafe {
                let c = &*ptr;
                let matches =
                    std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
                        == b"perch-test-no-real-key";
                CredFree(ptr as _);
                matches
            }
        } else {
            false
        };
        vault.delete(&reference).unwrap();
        assert!(success && matches);
    }
}

#[cfg(windows)]
impl Secrets for SystemSecrets {
    fn get(&self, reference: &str) -> Result<String> {
        use windows_sys::Win32::Security::Credentials::*;
        let target: Vec<u16> = format!("Perch/model/{reference}\0")
            .encode_utf16()
            .collect();
        let mut ptr = std::ptr::null_mut();
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut ptr) } == 0 {
            return Err(Failure::new(
                "CREDENTIAL_READ",
                "密钥不存在或不可读取，请重新填写",
            ));
        }
        let value = unsafe {
            let c = &*ptr;
            let bytes = if c.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
            };
            CredFree(ptr as _);
            String::from_utf8(bytes)
        };
        value.map_err(|_| Failure::new("CREDENTIAL_READ", "密钥编码不受支持，请重新填写"))
    }
    fn put(&self, reference: &str, value: &str) -> Result<()> {
        use windows_sys::Win32::Security::Credentials::*;
        let target: Vec<u16> = format!("Perch/model/{reference}\0")
            .encode_utf16()
            .collect();
        let user: Vec<u16> = "Perch\0".encode_utf16().collect();
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_ptr() as _,
            CredentialBlobSize: value.len() as u32,
            CredentialBlob: value.as_ptr() as _,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: user.as_ptr() as _,
            ..unsafe { std::mem::zeroed() }
        };
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err(Failure::new(
                "CREDENTIAL_WRITE",
                "Windows 凭据保存失败，请检查当前用户的凭据服务后重试",
            ));
        }
        Ok(())
    }
    fn delete(&self, reference: &str) -> Result<()> {
        use windows_sys::Win32::{
            Foundation::{GetLastError, ERROR_NOT_FOUND},
            Security::Credentials::*,
        };
        let target: Vec<u16> = format!("Perch/model/{reference}\0")
            .encode_utf16()
            .collect();
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0
            && unsafe { GetLastError() } != ERROR_NOT_FOUND
        {
            return Err(Failure::new(
                "CREDENTIAL_DELETE",
                "旧凭据清理失败，请稍后重试",
            ));
        }
        Ok(())
    }
}
#[cfg(not(windows))]
impl Secrets for SystemSecrets {
    fn put(&self, _: &str, _: &str) -> Result<()> {
        Err(Failure::new(
            "UNSUPPORTED_PLATFORM",
            "当前仅支持 Windows 凭据服务",
        ))
    }
    fn delete(&self, _: &str) -> Result<()> {
        Err(Failure::new(
            "UNSUPPORTED_PLATFORM",
            "当前仅支持 Windows 凭据服务",
        ))
    }
}
