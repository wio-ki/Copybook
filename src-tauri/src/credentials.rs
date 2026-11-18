use crate::{
    backup::{base_endpoint, Connection},
    model::{CmdError, Result},
    platform::wide,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{ptr, slice};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::ERROR_NOT_FOUND,
        Security::Credentials::{
            CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW,
            CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        },
    },
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PasswordKind {
    Access,
    Encryption,
}

#[derive(Serialize)]
pub struct PasswordStatus {
    pub access: bool,
    pub encryption: bool,
}

fn unavailable() -> CmdError {
    CmdError::new("io", "Windows Credential Manager is unavailable")
}

fn target(connection: &Connection, kind: PasswordKind) -> Result<Vec<u16>> {
    let url = base_endpoint(connection)?;
    if connection.username.trim().is_empty() {
        return Err(CmdError::invalid("A WebDAV username is required"));
    }
    let mut hash = Sha256::new();
    hash.update((url.as_str().len() as u64).to_le_bytes());
    hash.update(url.as_str());
    hash.update(&connection.username);
    let kind = match kind {
        PasswordKind::Access => "access",
        PasswordKind::Encryption => "encryption",
    };
    Ok(wide(&format!(
        "io.github.wio-ki.copybook/webdav/{kind}/{:x}",
        hash.finalize()
    )))
}

struct Credential(*mut CREDENTIALW);

impl Credential {
    fn read(target: &[u16]) -> Result<Option<Self>> {
        let mut raw = ptr::null_mut();
        // SAFETY: target is NUL-terminated UTF-16 and raw is a valid output pointer.
        let result =
            unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut raw) };
        match result {
            Ok(()) if !raw.is_null() => Ok(Some(Self(raw))),
            Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(None),
            _ => Err(unavailable()),
        }
    }

    fn bytes(&mut self) -> &mut [u8] {
        // SAFETY: Windows owns the returned allocation until CredFree; this wrapper
        // has exclusive access and checks the pointer/size before borrowing the blob.
        unsafe {
            let Some(credential) = self.0.as_mut() else {
                return &mut [];
            };
            if credential.CredentialBlob.is_null() || credential.CredentialBlobSize == 0 {
                return &mut [];
            }
            slice::from_raw_parts_mut(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
        }
    }
}

impl Drop for Credential {
    fn drop(&mut self) {
        self.bytes().zeroize();
        // SAFETY: pointer came from successful CredReadW and has not been freed.
        unsafe { CredFree(self.0.cast()) };
    }
}

fn read_password(connection: &Connection, kind: PasswordKind) -> Result<Option<Zeroizing<String>>> {
    let target = target(connection, kind)?;
    let Some(mut credential) = Credential::read(&target)? else {
        return Ok(None);
    };
    let value = std::str::from_utf8(credential.bytes()).map_err(|_| unavailable())?;
    Ok(Some(Zeroizing::new(value.to_owned())))
}

pub fn status(connection: &Connection) -> Result<PasswordStatus> {
    Ok(PasswordStatus {
        access: Credential::read(&target(connection, PasswordKind::Access)?)?.is_some(),
        encryption: Credential::read(&target(connection, PasswordKind::Encryption)?)?.is_some(),
    })
}

pub fn save(
    connection: &Connection,
    kind: PasswordKind,
    value: Option<Zeroizing<String>>,
) -> Result<()> {
    let mut target = target(connection, kind)?;
    let Some(value) = value else {
        // SAFETY: target is NUL-terminated UTF-16.
        return match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
            Err(_) => Err(unavailable()),
        };
    };
    let range = match kind {
        PasswordKind::Access => 1..=CRED_MAX_CREDENTIAL_BLOB_SIZE as usize,
        PasswordKind::Encryption => 8..=1024,
    };
    if !range.contains(&value.len()) {
        return Err(CmdError::invalid("Invalid credential length"));
    }
    let mut bytes = Zeroizing::new(value.as_bytes().to_vec());
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        CredentialBlobSize: bytes.len() as u32,
        CredentialBlob: bytes.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    // SAFETY: all borrowed buffers remain valid for the synchronous call.
    unsafe { CredWriteW(&credential, 0) }.map_err(|_| unavailable())
}

pub fn resolve_connection(mut connection: Connection) -> Result<Connection> {
    if connection.password.is_empty() {
        if let Some(mut password) = read_password(&connection, PasswordKind::Access)? {
            std::mem::swap(&mut connection.password, &mut *password);
        }
    }
    Ok(connection)
}

pub fn resolve_encryption(
    connection: &Connection,
    password: Zeroizing<String>,
) -> Result<Zeroizing<String>> {
    if password.is_empty() {
        Ok(read_password(connection, PasswordKind::Encryption)?.unwrap_or(password))
    } else {
        Ok(password)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(owner: &str) -> Connection {
        Connection {
            url: format!(
                "https://example.invalid/copybook-test-{}",
                std::process::id()
            ),
            username: owner.into(),
            password: String::new(),
        }
    }

    struct Cleanup(Connection);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for kind in [PasswordKind::Access, PasswordKind::Encryption] {
                let _ = save(&self.0, kind, None);
            }
        }
    }

    #[test]
    fn credential_store_scopes_passwords_and_supports_overrides_and_removal() {
        let fixture_user = "copybook-test-user";
        let connection = Cleanup(fixture(fixture_user));
        let access = "fixture access password";
        let encryption = "fixture 加密 password";
        for (kind, value) in [
            (PasswordKind::Access, access),
            (PasswordKind::Encryption, encryption),
        ] {
            save(&connection.0, kind, Some(Zeroizing::new(value.into()))).unwrap();
        }
        let saved = status(&connection.0).unwrap();
        assert!(saved.access && saved.encryption);
        let mut equivalent = fixture(fixture_user);
        equivalent.url.push('/');
        assert!(resolve_connection(equivalent).unwrap().password == access);
        assert!(
            resolve_encryption(&connection.0, Zeroizing::new(String::new()))
                .unwrap()
                .as_str()
                == encryption
        );
        assert!(
            resolve_encryption(&connection.0, Zeroizing::new("one-off password".into()))
                .unwrap()
                .as_str()
                == "one-off password"
        );
        let mut explicit = fixture(fixture_user);
        explicit.password = "one-off access password".into();
        assert!(resolve_connection(explicit).unwrap().password == "one-off access password");
        assert!(!status(&fixture("different-user")).unwrap().access);
        let mut different_url = fixture(fixture_user);
        different_url.url.push_str("-different");
        assert!(!status(&different_url).unwrap().encryption);
        save(&connection.0, PasswordKind::Access, None).unwrap();
        save(&connection.0, PasswordKind::Access, None).unwrap();
        let saved = status(&connection.0).unwrap();
        assert!(!saved.access && saved.encryption);
    }
}
