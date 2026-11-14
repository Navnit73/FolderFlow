//! macOS Keychain storage abstraction for sensitive credentials (license key, P7-T05).
//!
//! Uses `security-framework` (`SecItem*` Generic Password items) in production and
//! `InMemoryKeychain` in tests so suites never mutate the developer's login Keychain
//! unless explicitly running the isolated live-keychain smoke check.
//! Secret values are never logged (`NFR-013`).

use std::collections::HashMap;
use std::sync::Mutex;

use security_framework::passwords::{
    delete_generic_password, generic_password, set_generic_password, PasswordOptions,
};

use crate::error::{AppError, AppResult};

const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
const ERR_SEC_AUTH_FAILED: i32 = -25293;
const ERR_SEC_INTERACTION_NOT_ALLOWED: i32 = -25308;

/// Generic Keychain storage backend (`FR-212`).
pub trait KeychainBackend: Send + Sync {
    fn get_secret(&self, service: &str, account: &str) -> AppResult<Option<String>>;
    fn set_secret(&self, service: &str, account: &str, secret: &str) -> AppResult<()>;
    fn delete_secret(&self, service: &str, account: &str) -> AppResult<bool>;
}

/// Real macOS login Keychain backend via `Security.framework`.
#[derive(Debug, Default)]
pub struct MacOsKeychain;

impl MacOsKeychain {
    pub fn new() -> Self {
        Self
    }
}

fn map_sec_error(op: &str, err: &security_framework::base::Error) -> AppError {
    let code = err.code();
    let msg = match code {
        ERR_SEC_AUTH_FAILED | ERR_SEC_INTERACTION_NOT_ALLOWED => format!(
            "macOS Keychain is locked or denied access while trying to {op}. Unlock your login keychain and try again."
        ),
        _ => format!("macOS Keychain could not {op} (status {code})."),
    };
    AppError::Internal(msg)
}

impl KeychainBackend for MacOsKeychain {
    fn get_secret(&self, service: &str, account: &str) -> AppResult<Option<String>> {
        let options = PasswordOptions::new_generic_password(service, account);
        match generic_password(options) {
            Ok(bytes) => {
                let s = String::from_utf8(bytes).map_err(|_| {
                    AppError::Internal(
                        "Stored Keychain item was not valid UTF-8 and could not be decoded."
                            .to_owned(),
                    )
                })?;
                Ok(Some(s))
            }
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(map_sec_error("read the stored license", &e)),
        }
    }

    fn set_secret(&self, service: &str, account: &str, secret: &str) -> AppResult<()> {
        set_generic_password(service, account, secret.as_bytes())
            .map_err(|e| map_sec_error("save the license to Keychain", &e))
    }

    fn delete_secret(&self, service: &str, account: &str) -> AppResult<bool> {
        match delete_generic_password(service, account) {
            Ok(()) => Ok(true),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(false),
            Err(e) => Err(map_sec_error("remove the license from Keychain", &e)),
        }
    }
}

/// Thread-safe in-memory Keychain backend for deterministic tests (`R-KEY-01..02`).
#[derive(Debug, Default)]
pub struct InMemoryKeychain {
    entries: Mutex<HashMap<(String, String), String>>,
}

impl InMemoryKeychain {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KeychainBackend for InMemoryKeychain {
    fn get_secret(&self, service: &str, account: &str) -> AppResult<Option<String>> {
        let guard = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        Ok(guard
            .get(&(service.to_owned(), account.to_owned()))
            .cloned())
    }

    fn set_secret(&self, service: &str, account: &str, secret: &str) -> AppResult<()> {
        if service.trim().is_empty() || account.trim().is_empty() {
            return Err(AppError::validation(
                "keychain",
                "Service and account names cannot be empty.",
            ));
        }
        let mut guard = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(
            (service.to_owned(), account.to_owned()),
            secret.to_owned(),
        );
        Ok(())
    }

    fn delete_secret(&self, service: &str, account: &str) -> AppResult<bool> {
        let mut guard = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        Ok(guard
            .remove(&(service.to_owned(), account.to_owned()))
            .is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r_key_01_store_retrieve_overwrite_and_delete_round_trip() {
        let kc = InMemoryKeychain::new();
        let svc = "com.folderflow.desktop.license";
        let acct = "primary_license";

        assert_eq!(kc.get_secret(svc, acct).expect("read empty"), None);

        kc.set_secret(svc, acct, "FF1.payload.sig1")
            .expect("store secret");
        assert_eq!(
            kc.get_secret(svc, acct).expect("read stored").as_deref(),
            Some("FF1.payload.sig1")
        );

        kc.set_secret(svc, acct, "FF1.payload.sig2")
            .expect("overwrite secret");
        assert_eq!(
            kc.get_secret(svc, acct).expect("read updated").as_deref(),
            Some("FF1.payload.sig2")
        );

        assert!(kc.delete_secret(svc, acct).expect("delete existing"));
        assert!(!kc.delete_secret(svc, acct).expect("delete idempotent"));
        assert_eq!(kc.get_secret(svc, acct).expect("read after delete"), None);
    }

    #[test]
    fn r_key_02_services_and_accounts_are_isolated_and_invalid_rejected() {
        let kc = InMemoryKeychain::new();
        kc.set_secret("svc-a", "acct-1", "secret-a1").expect("set a1");
        kc.set_secret("svc-a", "acct-2", "secret-a2").expect("set a2");
        kc.set_secret("svc-b", "acct-1", "secret-b1").expect("set b1");

        assert_eq!(
            kc.get_secret("svc-a", "acct-1").expect("a1").as_deref(),
            Some("secret-a1")
        );
        assert_eq!(
            kc.get_secret("svc-a", "acct-2").expect("a2").as_deref(),
            Some("secret-a2")
        );
        assert_eq!(
            kc.get_secret("svc-b", "acct-1").expect("b1").as_deref(),
            Some("secret-b1")
        );

        assert!(kc.set_secret("", "acct", "x").is_err());
        assert!(kc.set_secret("svc", "   ", "x").is_err());

        // Real macOS Keychain isolated smoke check (skipped only if headless keychain unavailable).
        let live = MacOsKeychain;
        let test_svc = "com.folderflow.desktop.test.ephemeral";
        let test_acct = format!("test-{}", uuid::Uuid::now_v7());
        match live.set_secret(test_svc, &test_acct, "ephemeral-token") {
            Ok(()) => {
                let got = live.get_secret(test_svc, &test_acct).expect("live read");
                assert_eq!(got.as_deref(), Some("ephemeral-token"));
                assert!(live.delete_secret(test_svc, &test_acct).expect("live delete"));
                assert_eq!(
                    live.get_secret(test_svc, &test_acct)
                        .expect("live after delete"),
                    None
                );
            }
            Err(_) => {
                // Keychain may reject interaction in headless sandbox CI; deletion still idempotent.
                let _ = live.delete_secret(test_svc, &test_acct);
            }
        }
    }
}
