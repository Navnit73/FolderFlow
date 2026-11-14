//! License verification and Keychain-backed activation service (`FR-210..215`, `P7-T06`).
//!
//! Offline license keys and `.fflicense` files carry an Ed25519 signature over a
//! domain-separated canonical payload so activation works offline forever with zero
//! dependency on any license server (`FR-213`, `NFR-010`). Online activation uses a
//! provider-agnostic [`LicenseProvider`] trait (`Q1`) that only hits the network on
//! explicit user action (`activate` / `deactivate` / `validate`).

use std::future::Future;
use std::io::Read;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, AppResult};
use crate::security::keychain::KeychainBackend;
use crate::utils::clock::Clock;

pub const KEYCHAIN_SERVICE: &str = "com.folderflow.desktop.license";
pub const KEYCHAIN_ACCOUNT: &str = "primary_license";
pub const MAX_LICENSE_FILE_BYTES: u64 = 64 * 1024;
const LICENSE_SIGNING_DOMAIN: &[u8] = b"folderflow-license-v1\n";
const OFFLINE_KEY_PREFIX: &str = "FF1.";
const LICENSE_FILE_FORMAT: &str = "folderflow-license-v1";

/// Embedded default Ed25519 verifying key (overridable at compile time via
/// `FOLDERFLOW_LICENSE_PUBLIC_KEY_B64` or via [`LicenseService::with_verifying_key`] in tests).
const EMBEDDED_PUBLIC_KEY_BYTES: [u8; 32] = [
    103, 134, 211, 164, 187, 143, 163, 45, 64, 231, 226, 72, 246, 253, 251, 117, 211, 229, 253,
    222, 44, 20, 225, 66, 117, 245, 51, 43, 63, 214, 103, 41,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LicenseEdition {
    Lifetime,
    Pro,
    Trial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LicenseActivationSource {
    OfflineKey,
    OnlineProvider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LicenseStateKind {
    Unlicensed,
    ActiveOffline,
    ActiveOnline,
    Expired,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedLicensePayload {
    pub version: u32,
    pub license_id: String,
    pub edition: LicenseEdition,
    #[serde(default)]
    pub customer_hint: Option<String>,
    pub issued_at_ms: i64,
    #[serde(default)]
    pub expires_at_ms: Option<i64>,
    #[serde(default)]
    pub max_devices: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedLicenseFile {
    pub format: String,
    pub payload_b64: String,
    pub signature_b64: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LicenseStatus {
    pub state: LicenseStateKind,
    pub licensed: bool,
    pub edition: Option<LicenseEdition>,
    pub source: Option<LicenseActivationSource>,
    pub license_id: Option<String>,
    pub masked_key: Option<String>,
    pub customer_hint: Option<String>,
    pub provider_name: Option<String>,
    pub online_provider_configured: bool,
    pub device_limit: Option<u32>,
    #[specta(type = Option<f64>)]
    pub activated_at_ms: Option<i64>,
    #[specta(type = Option<f64>)]
    pub expires_at_ms: Option<i64>,
    pub last_error: Option<String>,
}

impl LicenseStatus {
    fn unlicensed(online_provider_configured: bool, provider_name: Option<String>) -> Self {
        Self {
            state: LicenseStateKind::Unlicensed,
            licensed: false,
            edition: None,
            source: None,
            license_id: None,
            masked_key: None,
            customer_hint: None,
            provider_name,
            online_provider_configured,
            device_limit: None,
            activated_at_ms: None,
            expires_at_ms: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredLicenseRecord {
    source: LicenseActivationSource,
    masked_key: String,
    raw_token_or_key: String,
    license_id: String,
    edition: LicenseEdition,
    customer_hint: Option<String>,
    provider_name: Option<String>,
    instance_id: Option<String>,
    device_limit: Option<u32>,
    activated_at_ms: i64,
    expires_at_ms: Option<i64>,
}

/// Device metadata sent only during explicit online provider activation (`FR-215`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceContext {
    pub device_name: String,
    pub platform: &'static str,
    pub app_version: &'static str,
}

impl DeviceContext {
    pub fn current() -> Self {
        Self {
            device_name: "Mac".to_owned(),
            platform: "macos",
            app_version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderActivationResult {
    pub license_id: String,
    pub edition: LicenseEdition,
    pub customer_hint: Option<String>,
    pub instance_id: Option<String>,
    pub device_limit: Option<u32>,
    pub expires_at_ms: Option<i64>,
    /// Optional provider-issued Ed25519 offline token for offline resilience.
    pub signed_token: Option<String>,
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Provider-agnostic licensing adapter (`Q1`, `NFR-010`).
pub trait LicenseProvider: Send + Sync {
    fn provider_name(&self) -> Option<&str>;
    fn is_configured(&self) -> bool;

    fn activate<'a>(
        &'a self,
        license_key: &'a str,
        device: &'a DeviceContext,
    ) -> BoxFuture<'a, AppResult<ProviderActivationResult>>;

    fn deactivate<'a>(
        &'a self,
        license_id: &'a str,
        instance_id: Option<&'a str>,
        license_key: &'a str,
    ) -> BoxFuture<'a, AppResult<()>>;
}

/// Default provider used until a hosted licensing vendor is chosen (`Q1`).
/// Offline Ed25519 signed keys and `.fflicense` files work with no online provider.
#[derive(Debug, Default)]
pub struct UnconfiguredOnlineProvider;

impl LicenseProvider for UnconfiguredOnlineProvider {
    fn provider_name(&self) -> Option<&str> {
        None
    }

    fn is_configured(&self) -> bool {
        false
    }

    fn activate<'a>(
        &'a self,
        _license_key: &'a str,
        _device: &'a DeviceContext,
    ) -> BoxFuture<'a, AppResult<ProviderActivationResult>> {
        Box::pin(async {
            Err(AppError::validation(
                "licenseKey",
                "Online activation provider is not configured in this build yet. Paste an offline signed license key (FF1…) or import a .fflicense file.",
            ))
        })
    }

    fn deactivate<'a>(
        &'a self,
        _license_id: &'a str,
        _instance_id: Option<&'a str>,
        _license_key: &'a str,
    ) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Debug, Clone)]
pub struct HttpProviderConfig {
    pub provider_name: String,
    pub activate_url: String,
    pub deactivate_url: String,
    pub api_token: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HttpActivateRequest<'a> {
    license_key: &'a str,
    device_name: &'a str,
    platform: &'static str,
    app_version: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HttpDeactivateRequest<'a> {
    license_id: &'a str,
    instance_id: Option<&'a str>,
    license_key: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HttpActivateResponse {
    activated: bool,
    license_id: Option<String>,
    edition: Option<LicenseEdition>,
    customer_hint: Option<String>,
    instance_id: Option<String>,
    device_limit: Option<u32>,
    expires_at_ms: Option<i64>,
    signed_token: Option<String>,
    error: Option<String>,
}

/// Configurable HTTPS/JSON license provider (`NFR-010`: called only on user action).
pub struct HttpJsonLicenseProvider {
    config: HttpProviderConfig,
    client: reqwest::Client,
}

impl HttpJsonLicenseProvider {
    pub fn new(config: HttpProviderConfig) -> AppResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AppError::Internal(format!("HTTP license client init failed: {e}")))?;
        Ok(Self { config, client })
    }
}

impl LicenseProvider for HttpJsonLicenseProvider {
    fn provider_name(&self) -> Option<&str> {
        Some(&self.config.provider_name)
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn activate<'a>(
        &'a self,
        license_key: &'a str,
        device: &'a DeviceContext,
    ) -> BoxFuture<'a, AppResult<ProviderActivationResult>> {
        Box::pin(async move {
            let mut req = self
                .client
                .post(&self.config.activate_url)
                .json(&HttpActivateRequest {
                    license_key,
                    device_name: &device.device_name,
                    platform: device.platform,
                    app_version: device.app_version,
                });
            if let Some(tok) = &self.config.api_token {
                req = req.bearer_auth(tok);
            }
            let resp = req.send().await.map_err(|e| {
                tracing::warn!(target: "folderflow::license", error = %e, "license server unreachable");
                AppError::validation(
                    "licenseKey",
                    "Could not reach the license server. Check your internet connection or use an offline signed license key.",
                )
            })?;
            let status = resp.status();
            let body: HttpActivateResponse = resp.json().await.map_err(|_| {
                AppError::validation(
                    "licenseKey",
                    "License server returned an unreadable response.",
                )
            })?;
            if !status.is_success() || !body.activated {
                return Err(AppError::validation(
                    "licenseKey",
                    body.error.unwrap_or_else(|| {
                        format!(
                            "License activation was rejected (HTTP {}).",
                            status.as_u16()
                        )
                    }),
                ));
            }
            let license_id = body.license_id.ok_or_else(|| {
                AppError::validation("licenseKey", "License server response omitted license ID.")
            })?;
            Ok(ProviderActivationResult {
                license_id,
                edition: body.edition.unwrap_or(LicenseEdition::Lifetime),
                customer_hint: body.customer_hint,
                instance_id: body.instance_id,
                device_limit: body.device_limit,
                expires_at_ms: body.expires_at_ms,
                signed_token: body.signed_token,
            })
        })
    }

    fn deactivate<'a>(
        &'a self,
        license_id: &'a str,
        instance_id: Option<&'a str>,
        license_key: &'a str,
    ) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move {
            let mut req =
                self.client
                    .post(&self.config.deactivate_url)
                    .json(&HttpDeactivateRequest {
                        license_id,
                        instance_id,
                        license_key,
                    });
            if let Some(tok) = &self.config.api_token {
                req = req.bearer_auth(tok);
            }
            let resp = req.send().await.map_err(|e| {
                tracing::warn!(target: "folderflow::license", error = %e, "license deactivation server unreachable");
                AppError::validation(
                    "licenseKey",
                    "Could not reach the license server to release this device slot.",
                )
            })?;
            if !resp.status().is_success() {
                return Err(AppError::validation(
                    "licenseKey",
                    format!(
                        "License server rejected deactivation (HTTP {}).",
                        resp.status().as_u16()
                    ),
                ));
            }
            Ok(())
        })
    }
}

pub struct LicenseService {
    keychain: Arc<dyn KeychainBackend>,
    provider: Arc<dyn LicenseProvider>,
    clock: Arc<dyn Clock>,
    verifying_key: VerifyingKey,
}

impl LicenseService {
    pub fn new(
        keychain: Arc<dyn KeychainBackend>,
        provider: Arc<dyn LicenseProvider>,
        clock: Arc<dyn Clock>,
    ) -> AppResult<Self> {
        let vk_bytes = resolve_embedded_public_key_bytes()?;
        let verifying_key = VerifyingKey::from_bytes(&vk_bytes).map_err(|e| {
            AppError::Internal(format!("Invalid embedded license verifying key: {e}"))
        })?;
        Ok(Self {
            keychain,
            provider,
            clock,
            verifying_key,
        })
    }

    pub fn with_verifying_key(
        keychain: Arc<dyn KeychainBackend>,
        provider: Arc<dyn LicenseProvider>,
        clock: Arc<dyn Clock>,
        verifying_key: VerifyingKey,
    ) -> Self {
        Self {
            keychain,
            provider,
            clock,
            verifying_key,
        }
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.verifying_key
    }

    /// Reads and validates current license status locally from macOS Keychain (`FR-213`:
    /// strictly zero network calls).
    pub fn status(&self) -> AppResult<LicenseStatus> {
        let online_cfg = self.provider.is_configured();
        let provider_name = self.provider.provider_name().map(str::to_owned);
        let Some(raw_json) = self
            .keychain
            .get_secret(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)?
        else {
            return Ok(LicenseStatus::unlicensed(online_cfg, provider_name));
        };

        let Ok(stored) = serde_json::from_str::<StoredLicenseRecord>(&raw_json) else {
            return Ok(LicenseStatus {
                state: LicenseStateKind::Invalid,
                licensed: false,
                edition: None,
                source: None,
                license_id: None,
                masked_key: None,
                customer_hint: None,
                provider_name,
                online_provider_configured: online_cfg,
                device_limit: None,
                activated_at_ms: None,
                expires_at_ms: None,
                last_error: Some(
                    "Stored license entry could not be decoded. Re-activate your license."
                        .to_owned(),
                ),
            });
        };

        match stored.source {
            LicenseActivationSource::OfflineKey => {
                match self.verify_offline_input(&stored.raw_token_or_key) {
                    Ok(payload) => {
                        if let Some(exp) = payload.expires_at_ms {
                            if payload.edition != LicenseEdition::Lifetime
                                && self.clock.now_ms() > exp
                            {
                                return Ok(LicenseStatus {
                                    state: LicenseStateKind::Expired,
                                    licensed: false,
                                    edition: Some(payload.edition),
                                    source: Some(LicenseActivationSource::OfflineKey),
                                    license_id: Some(payload.license_id),
                                    masked_key: Some(stored.masked_key),
                                    customer_hint: payload.customer_hint,
                                    provider_name,
                                    online_provider_configured: online_cfg,
                                    device_limit: payload.max_devices,
                                    activated_at_ms: Some(stored.activated_at_ms),
                                    expires_at_ms: payload.expires_at_ms,
                                    last_error: Some("Your license has expired.".to_owned()),
                                });
                            }
                        }
                        Ok(LicenseStatus {
                            state: LicenseStateKind::ActiveOffline,
                            licensed: true,
                            edition: Some(payload.edition),
                            source: Some(LicenseActivationSource::OfflineKey),
                            license_id: Some(payload.license_id),
                            masked_key: Some(stored.masked_key),
                            customer_hint: payload.customer_hint,
                            provider_name,
                            online_provider_configured: online_cfg,
                            device_limit: payload.max_devices,
                            activated_at_ms: Some(stored.activated_at_ms),
                            expires_at_ms: if payload.edition == LicenseEdition::Lifetime {
                                None
                            } else {
                                payload.expires_at_ms
                            },
                            last_error: None,
                        })
                    }
                    Err(err) => Ok(LicenseStatus {
                        state: LicenseStateKind::Invalid,
                        licensed: false,
                        edition: Some(stored.edition),
                        source: Some(LicenseActivationSource::OfflineKey),
                        license_id: Some(stored.license_id),
                        masked_key: Some(stored.masked_key),
                        customer_hint: stored.customer_hint,
                        provider_name,
                        online_provider_configured: online_cfg,
                        device_limit: stored.device_limit,
                        activated_at_ms: Some(stored.activated_at_ms),
                        expires_at_ms: stored.expires_at_ms,
                        last_error: Some(err.to_string()),
                    }),
                }
            }
            LicenseActivationSource::OnlineProvider => {
                if let Some(exp) = stored.expires_at_ms {
                    if stored.edition != LicenseEdition::Lifetime && self.clock.now_ms() > exp {
                        return Ok(LicenseStatus {
                            state: LicenseStateKind::Expired,
                            licensed: false,
                            edition: Some(stored.edition),
                            source: Some(LicenseActivationSource::OnlineProvider),
                            license_id: Some(stored.license_id),
                            masked_key: Some(stored.masked_key),
                            customer_hint: stored.customer_hint,
                            provider_name: stored.provider_name.or(provider_name),
                            online_provider_configured: online_cfg,
                            device_limit: stored.device_limit,
                            activated_at_ms: Some(stored.activated_at_ms),
                            expires_at_ms: stored.expires_at_ms,
                            last_error: Some("Your license has expired.".to_owned()),
                        });
                    }
                }
                Ok(LicenseStatus {
                    state: LicenseStateKind::ActiveOnline,
                    licensed: true,
                    edition: Some(stored.edition),
                    source: Some(LicenseActivationSource::OnlineProvider),
                    license_id: Some(stored.license_id),
                    masked_key: Some(stored.masked_key),
                    customer_hint: stored.customer_hint,
                    provider_name: stored.provider_name.or(provider_name),
                    online_provider_configured: online_cfg,
                    device_limit: stored.device_limit,
                    activated_at_ms: Some(stored.activated_at_ms),
                    expires_at_ms: if stored.edition == LicenseEdition::Lifetime {
                        None
                    } else {
                        stored.expires_at_ms
                    },
                    last_error: None,
                })
            }
        }
    }

    /// Validates and activates an offline Ed25519-signed license key (`FF1.<payload>.<sig>`)
    /// or `.fflicense` JSON document, persisting it to Keychain (`FR-211`, `FR-212`).
    pub fn activate_offline(&self, signed_input: &str) -> AppResult<LicenseStatus> {
        let trimmed = sanitize_license_input(signed_input);
        let payload = self.verify_offline_input(trimmed)?;
        if let Some(exp) = payload.expires_at_ms {
            if payload.edition != LicenseEdition::Lifetime && self.clock.now_ms() > exp {
                return Err(AppError::validation(
                    "licenseKey",
                    "This offline license key has expired.",
                ));
            }
        }

        let record = StoredLicenseRecord {
            source: LicenseActivationSource::OfflineKey,
            masked_key: mask_license_key(&payload.license_id),
            raw_token_or_key: trimmed.to_owned(),
            license_id: payload.license_id,
            edition: payload.edition,
            customer_hint: payload.customer_hint,
            provider_name: None,
            instance_id: None,
            device_limit: payload.max_devices,
            activated_at_ms: self.clock.now_ms(),
            expires_at_ms: if payload.edition == LicenseEdition::Lifetime {
                None
            } else {
                payload.expires_at_ms
            },
        };
        self.store_record(&record)?;
        self.status()
    }

    /// Reads a user-chosen `.fflicense` file safely (non-symlink, bounded `<= 64 KiB`) and
    /// activates it offline (`FR-211`).
    pub fn activate_offline_file(&self, path: &Path) -> AppResult<LicenseStatus> {
        let meta = std::fs::symlink_metadata(path).map_err(|e| {
            AppError::validation("licenseFile", format!("Could not read license file: {e}"))
        })?;
        if meta.file_type().is_symlink() || !meta.file_type().is_file() {
            return Err(AppError::validation(
                "licenseFile",
                "License file must be a regular file (symlinks are not allowed).",
            ));
        }
        if meta.len() > MAX_LICENSE_FILE_BYTES {
            return Err(AppError::validation(
                "licenseFile",
                "License file exceeds the 64 KiB maximum size.",
            ));
        }
        let file = std::fs::File::open(path)?;
        let mut buf = String::new();
        file.take(MAX_LICENSE_FILE_BYTES + 1)
            .read_to_string(&mut buf)?;
        if buf.len() as u64 > MAX_LICENSE_FILE_BYTES {
            return Err(AppError::validation(
                "licenseFile",
                "License file exceeds the 64 KiB maximum size.",
            ));
        }
        self.activate_offline(&buf)
    }

    /// Activates via the configured online license provider (`NFR-010`: user action only).
    /// Also detects offline signed tokens pasted into the standard key field automatically.
    pub async fn activate(&self, license_key: &str) -> AppResult<LicenseStatus> {
        let trimmed = sanitize_license_input(license_key);
        if trimmed.is_empty() {
            return Err(AppError::validation(
                "licenseKey",
                "Enter a license key before activating.",
            ));
        }
        if looks_like_offline_token(trimmed) {
            return self.activate_offline(trimmed);
        }

        let device = DeviceContext::current();
        let res = self.provider.activate(trimmed, &device).await?;
        if let Some(signed_token) = res.signed_token.as_deref() {
            let payload = self.verify_offline_input(signed_token)?;
            let record = StoredLicenseRecord {
                source: LicenseActivationSource::OfflineKey,
                masked_key: mask_license_key(trimmed),
                raw_token_or_key: signed_token.trim().to_owned(),
                license_id: payload.license_id,
                edition: payload.edition,
                customer_hint: payload.customer_hint.or(res.customer_hint),
                provider_name: self.provider.provider_name().map(str::to_owned),
                instance_id: res.instance_id,
                device_limit: payload.max_devices.or(res.device_limit),
                activated_at_ms: self.clock.now_ms(),
                expires_at_ms: if payload.edition == LicenseEdition::Lifetime {
                    None
                } else {
                    payload.expires_at_ms
                },
            };
            self.store_record(&record)?;
            return self.status();
        }

        let record = StoredLicenseRecord {
            source: LicenseActivationSource::OnlineProvider,
            masked_key: mask_license_key(trimmed),
            raw_token_or_key: trimmed.to_owned(),
            license_id: res.license_id,
            edition: res.edition,
            customer_hint: res.customer_hint,
            provider_name: self.provider.provider_name().map(str::to_owned),
            instance_id: res.instance_id,
            device_limit: res.device_limit,
            activated_at_ms: self.clock.now_ms(),
            expires_at_ms: if res.edition == LicenseEdition::Lifetime {
                None
            } else {
                res.expires_at_ms
            },
        };
        self.store_record(&record)?;
        self.status()
    }

    /// Releases an online activation slot when reachable and removes the Keychain item (`FR-210`).
    /// Removal from Keychain still succeeds locally if offline, unless provider strictly fails.
    pub async fn deactivate(&self) -> AppResult<LicenseStatus> {
        if let Some(raw_json) = self
            .keychain
            .get_secret(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)?
        {
            if let Ok(stored) = serde_json::from_str::<StoredLicenseRecord>(&raw_json) {
                if stored.source == LicenseActivationSource::OnlineProvider
                    && self.provider.is_configured()
                {
                    if let Err(err) = self
                        .provider
                        .deactivate(
                            &stored.license_id,
                            stored.instance_id.as_deref(),
                            &stored.raw_token_or_key,
                        )
                        .await
                    {
                        tracing::warn!(
                            target: "folderflow::license",
                            error = %err,
                            "online provider deactivation failed; removing local keychain credential"
                        );
                    }
                }
            }
        }
        self.keychain
            .delete_secret(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)?;
        self.status()
    }

    fn store_record(&self, record: &StoredLicenseRecord) -> AppResult<()> {
        let json = serde_json::to_string(record)?;
        self.keychain
            .set_secret(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, &json)
    }

    pub fn verify_offline_input(&self, input: &str) -> AppResult<SignedLicensePayload> {
        let trimmed = sanitize_license_input(input);
        if trimmed.is_empty() {
            return Err(AppError::validation(
                "licenseKey",
                "Offline license key cannot be empty.",
            ));
        }

        let (payload_b64, sig_b64) = if trimmed.starts_with('{') {
            let file: SignedLicenseFile = serde_json::from_str(trimmed).map_err(|_| {
                AppError::validation("licenseKey", "The offline license file is not valid JSON.")
            })?;
            if file.format != LICENSE_FILE_FORMAT {
                return Err(AppError::validation(
                    "licenseKey",
                    "Unsupported license file format.",
                ));
            }
            (file.payload_b64, file.signature_b64)
        } else {
            let compacted: String = trimmed.chars().filter(|c| !is_strip_char(*c)).collect();
            let Some(rest) = compacted.strip_prefix(OFFLINE_KEY_PREFIX) else {
                return Err(AppError::validation(
                    "licenseKey",
                    "Invalid offline license key format. Expected a key starting with FF1.",
                ));
            };
            let mut parts = rest.split('.');
            let (Some(p), Some(s), None) = (parts.next(), parts.next(), parts.next()) else {
                return Err(AppError::validation(
                    "licenseKey",
                    "Malformed offline license key segments.",
                ));
            };
            (p.to_owned(), s.to_owned())
        };

        let payload_bytes = URL_SAFE_NO_PAD
            .decode(payload_b64.as_bytes())
            .map_err(|_| AppError::validation("licenseKey", "Corrupted license key payload."))?;
        let sig_bytes = URL_SAFE_NO_PAD
            .decode(sig_b64.as_bytes())
            .map_err(|_| AppError::validation("licenseKey", "Corrupted license key signature."))?;
        let sig_arr: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| AppError::validation("licenseKey", "Invalid Ed25519 signature length."))?;
        let signature = Signature::from_bytes(&sig_arr);

        let mut signed_msg = Vec::with_capacity(LICENSE_SIGNING_DOMAIN.len() + payload_bytes.len());
        signed_msg.extend_from_slice(LICENSE_SIGNING_DOMAIN);
        signed_msg.extend_from_slice(&payload_bytes);

        self.verifying_key
            .verify_strict(&signed_msg, &signature)
            .map_err(|_| {
                AppError::validation(
                    "licenseKey",
                    "License signature verification failed. The key may be invalid or tampered.",
                )
            })?;

        let payload: SignedLicensePayload =
            serde_json::from_slice(&payload_bytes).map_err(|_| {
                AppError::validation("licenseKey", "License payload could not be parsed.")
            })?;
        if payload.version != 1 || payload.license_id.trim().is_empty() {
            return Err(AppError::validation(
                "licenseKey",
                "License payload version or identifier is invalid.",
            ));
        }
        Ok(payload)
    }
}

fn is_strip_char(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{feff}' | '\u{200b}' | '\u{200c}' | '\u{200d}')
}

fn sanitize_license_input(s: &str) -> &str {
    s.trim_matches(is_strip_char)
}

fn looks_like_offline_token(s: &str) -> bool {
    let trimmed = sanitize_license_input(s);
    trimmed.starts_with(OFFLINE_KEY_PREFIX) || trimmed.starts_with('{')
}

fn resolve_embedded_public_key_bytes() -> AppResult<[u8; 32]> {
    if let Some(b64) = option_env!("FOLDERFLOW_LICENSE_PUBLIC_KEY_B64") {
        let decoded = STANDARD.decode(b64.trim()).map_err(|_| {
            AppError::Internal("Invalid FOLDERFLOW_LICENSE_PUBLIC_KEY_B64 build variable".into())
        })?;
        let arr: [u8; 32] = decoded.try_into().map_err(|_| {
            AppError::Internal("FOLDERFLOW_LICENSE_PUBLIC_KEY_B64 must be 32 bytes".into())
        })?;
        return Ok(arr);
    }
    Ok(EMBEDDED_PUBLIC_KEY_BYTES)
}

/// Masks a key/license ID for display (`FF1-••••-ABCD`), never leaking full secret tokens.
pub fn mask_license_key(raw: &str) -> String {
    let clean: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let tail: String = clean
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if clean.len() <= 6 {
        format!("••••-{tail}")
    } else {
        let head: String = clean.chars().take(4).collect();
        format!("{head}-••••-{tail}")
    }
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    pub fn sign_payload(signing_key: &SigningKey, payload: &SignedLicensePayload) -> String {
        let json = serde_json::to_vec(payload).expect("serialize payload");
        let mut msg = Vec::with_capacity(LICENSE_SIGNING_DOMAIN.len() + json.len());
        msg.extend_from_slice(LICENSE_SIGNING_DOMAIN);
        msg.extend_from_slice(&json);
        let sig = signing_key.sign(&msg);
        format!(
            "{OFFLINE_KEY_PREFIX}{}.{}",
            URL_SAFE_NO_PAD.encode(json),
            URL_SAFE_NO_PAD.encode(sig.to_bytes())
        )
    }

    pub fn sign_file_json(signing_key: &SigningKey, payload: &SignedLicensePayload) -> String {
        let json = serde_json::to_vec(payload).expect("serialize payload");
        let mut msg = Vec::with_capacity(LICENSE_SIGNING_DOMAIN.len() + json.len());
        msg.extend_from_slice(LICENSE_SIGNING_DOMAIN);
        msg.extend_from_slice(&json);
        let sig = signing_key.sign(&msg);
        serde_json::to_string_pretty(&SignedLicenseFile {
            format: LICENSE_FILE_FORMAT.to_owned(),
            payload_b64: URL_SAFE_NO_PAD.encode(json),
            signature_b64: URL_SAFE_NO_PAD.encode(sig.to_bytes()),
        })
        .expect("serialize license file")
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::test_helpers::{sign_file_json, sign_payload};
    use super::*;
    use crate::security::keychain::InMemoryKeychain;
    use crate::utils::clock::FixedClock;

    fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn lifetime_payload() -> SignedLicensePayload {
        SignedLicensePayload {
            version: 1,
            license_id: "FF-LIFE-2026-0001".to_owned(),
            edition: LicenseEdition::Lifetime,
            customer_hint: Some("alice@example.com".to_owned()),
            issued_at_ms: 1_700_000_000_000,
            expires_at_ms: Some(1_700_000_000_100), // explicitly ignored for Lifetime (R-LIC-03)
            max_devices: Some(3),
        }
    }

    fn build_service(
        now_ms: i64,
        provider: Arc<dyn LicenseProvider>,
    ) -> (LicenseService, SigningKey) {
        let sk = test_signing_key();
        let svc = LicenseService::with_verifying_key(
            Arc::new(InMemoryKeychain::new()),
            provider,
            Arc::new(FixedClock::utc(now_ms)),
            sk.verifying_key(),
        );
        (svc, sk)
    }

    #[test]
    fn r_lic_01_valid_ed25519_signed_key_and_file_activate_offline_and_persist_in_keychain() {
        let kc = Arc::new(InMemoryKeychain::new());
        let sk = test_signing_key();
        let svc = LicenseService::with_verifying_key(
            kc.clone(),
            Arc::new(UnconfiguredOnlineProvider),
            Arc::new(FixedClock::utc(1_750_000_000_000)),
            sk.verifying_key(),
        );

        let token = sign_payload(&sk, &lifetime_payload());
        let status = svc
            .activate_offline(&format!("  {token}\n  "))
            .expect("activate offline token");
        assert!(status.licensed);
        assert_eq!(status.state, LicenseStateKind::ActiveOffline);
        assert_eq!(status.edition, Some(LicenseEdition::Lifetime));
        assert_eq!(status.license_id.as_deref(), Some("FF-LIFE-2026-0001"));
        assert_eq!(status.expires_at_ms, None);

        // Fresh service instance reading the same Keychain reconstructs full active state offline.
        let svc2 = LicenseService::with_verifying_key(
            kc,
            Arc::new(UnconfiguredOnlineProvider),
            Arc::new(FixedClock::utc(1_750_000_000_000)),
            sk.verifying_key(),
        );
        let restored = svc2.status().expect("status from keychain");
        assert!(restored.licensed);
        assert_eq!(restored.state, LicenseStateKind::ActiveOffline);

        // Signed `.fflicense` JSON file also activates safely.
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("team.fflicense");
        crate::test_support::write_file(
            &file_path,
            sign_file_json(&sk, &lifetime_payload()).as_bytes(),
        );
        let file_status = svc2
            .activate_offline_file(&file_path)
            .expect("activate offline file");
        assert!(file_status.licensed);
    }

    #[test]
    fn r_lic_02_tampered_payload_wrong_key_or_corrupted_signature_rejected() {
        let (svc, sk) = build_service(1_750_000_000_000, Arc::new(UnconfiguredOnlineProvider));
        let valid = sign_payload(&sk, &lifetime_payload());

        // Tamper 1 character in payload segment.
        let mut parts: Vec<&str> = valid.split('.').collect();
        let mut payload_chars: Vec<char> = parts[1].chars().collect();
        payload_chars[0] = if payload_chars[0] == 'A' { 'B' } else { 'A' };
        let tampered_payload: String = payload_chars.into_iter().collect();
        parts[1] = &tampered_payload;
        let tampered_token = parts.join(".");
        assert!(svc.activate_offline(&tampered_token).is_err());
        assert!(!svc.status().expect("status").licensed);

        // Wrong signing key rejected.
        let other_sk = SigningKey::from_bytes(&[99u8; 32]);
        let wrong_key_token = sign_payload(&other_sk, &lifetime_payload());
        assert!(svc.activate_offline(&wrong_key_token).is_err());

        // Malformed prefix / oversized / symlink file rejected.
        assert!(svc.activate_offline("NOT-A-VALID-TOKEN").is_err());
        let dir = tempfile::tempdir().expect("tempdir");
        let huge = dir.path().join("huge.fflicense");
        crate::test_support::write_file(&huge, &vec![b'a'; (MAX_LICENSE_FILE_BYTES as usize) + 8]);
        assert!(svc.activate_offline_file(&huge).is_err());
    }

    #[test]
    fn r_lic_03_lifetime_license_has_no_expiry_and_timed_expiry_is_enforced() {
        let kc = Arc::new(InMemoryKeychain::new());
        let sk = test_signing_key();
        let svc = LicenseService::with_verifying_key(
            kc.clone(),
            Arc::new(UnconfiguredOnlineProvider),
            Arc::new(FixedClock::utc(2_000_000_000_000)),
            sk.verifying_key(),
        );

        // Lifetime succeeds far past any timestamp and clears `expires_at_ms`.
        let life_tok = sign_payload(&sk, &lifetime_payload());
        let st = svc.activate_offline(&life_tok).expect("lifetime activates");
        assert!(st.licensed);
        assert_eq!(st.expires_at_ms, None);

        // Trial license past `expires_at_ms` fails activation and transitions stored status to Expired.
        let expired_trial = SignedLicensePayload {
            version: 1,
            license_id: "FF-TRIAL-1".to_owned(),
            edition: LicenseEdition::Trial,
            customer_hint: None,
            issued_at_ms: 1_000,
            expires_at_ms: Some(5_000),
            max_devices: None,
        };
        let trial_tok = sign_payload(&sk, &expired_trial);
        assert!(svc.activate_offline(&trial_tok).is_err());

        let svc_before = LicenseService::with_verifying_key(
            kc.clone(),
            Arc::new(UnconfiguredOnlineProvider),
            Arc::new(FixedClock::utc(2_000)),
            sk.verifying_key(),
        );
        assert!(
            svc_before
                .activate_offline(&trial_tok)
                .expect("active trial")
                .licensed
        );

        let svc_after = LicenseService::with_verifying_key(
            kc,
            Arc::new(UnconfiguredOnlineProvider),
            Arc::new(FixedClock::utc(6_000)),
            sk.verifying_key(),
        );
        let expired_st = svc_after.status().expect("status after expiry");
        assert!(!expired_st.licensed);
        assert_eq!(expired_st.state, LicenseStateKind::Expired);
    }

    #[tokio::test]
    async fn r_lic_04_offline_works_and_status_stays_active_with_network_unreachable() {
        let provider = Arc::new(
            HttpJsonLicenseProvider::new(HttpProviderConfig {
                provider_name: "unreachable-provider".to_owned(),
                // Unreachable loopback port; tests offline resilience cleanly.
                activate_url: "http://127.0.0.1:9/activate".to_owned(),
                deactivate_url: "http://127.0.0.1:9/deactivate".to_owned(),
                api_token: None,
            })
            .expect("client"),
        );
        let (svc, sk) = build_service(1_750_000_000_000, provider);

        let token = sign_payload(&sk, &lifetime_payload());
        // Offline token passed even through main `activate` activates cleanly with zero network!
        let st = svc
            .activate(&token)
            .await
            .expect("offline key works via activate");
        assert!(st.licensed);
        assert_eq!(
            svc.status().expect("offline status").state,
            LicenseStateKind::ActiveOffline
        );

        // Online key when server is down surfaces a friendly message, without disrupting stored key.
        let err = svc
            .activate("ONLINE-XXXX-YYYY")
            .await
            .expect_err("server down");
        assert!(err.to_string().contains("Could not reach"));
        assert!(svc.status().expect("still licensed").licensed);
    }

    #[tokio::test]
    async fn r_lic_05_online_activation_stores_verified_token_and_surfaces_provider_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let sk = test_signing_key();
        let sk_clone = sk.clone();

        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.expect("accept");
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).await.expect("read");
                let req = String::from_utf8_lossy(&buf[..n]);
                let (status_line, json) = if req.contains("POST /activate")
                    && req.contains("BAD-KEY")
                {
                    (
                        "400 Bad Request",
                        serde_json::json!({ "activated": false, "error": "Activation limit reached for this license key." })
                            .to_string(),
                    )
                } else if req.contains("POST /activate") {
                    let signed = sign_payload(&sk_clone, &lifetime_payload());
                    serde_json::json!({
                        "activated": true,
                        "licenseId": "FF-LIFE-2026-0001",
                        "edition": "lifetime",
                        "instanceId": "inst-123",
                        "deviceLimit": 3,
                        "signedToken": signed
                    })
                    .to_string()
                    .pipe_ok()
                } else {
                    ("200 OK", "{}".to_owned())
                };
                let resp = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                    json.len()
                );
                stream.write_all(resp.as_bytes()).await.expect("write");
            }
        });

        let provider = Arc::new(
            HttpJsonLicenseProvider::new(HttpProviderConfig {
                provider_name: "test-http".to_owned(),
                activate_url: format!("http://{addr}/activate"),
                deactivate_url: format!("http://{addr}/deactivate"),
                api_token: None,
            })
            .expect("client"),
        );
        let svc = LicenseService::with_verifying_key(
            Arc::new(InMemoryKeychain::new()),
            provider,
            Arc::new(FixedClock::utc(1_750_000_000_000)),
            sk.verifying_key(),
        );

        let err = svc.activate("BAD-KEY").await.expect_err("bad key rejected");
        assert!(err.to_string().contains("Activation limit reached"));

        let st = svc
            .activate("GOOD-ONLINE-KEY-9999")
            .await
            .expect("good key activates");
        assert!(st.licensed);
        assert_eq!(st.device_limit, Some(3));
        assert_eq!(st.provider_name.as_deref(), Some("test-http"));
        assert_eq!(st.masked_key.as_deref(), Some("GOOD-••••-9999"));
        let _ = server.await;
    }

    #[tokio::test]
    async fn r_lic_06_deactivate_clears_keychain_even_when_offline_or_with_provider() {
        let provider = Arc::new(
            HttpJsonLicenseProvider::new(HttpProviderConfig {
                provider_name: "unreachable-provider".to_owned(),
                activate_url: "http://127.0.0.1:9/activate".to_owned(),
                deactivate_url: "http://127.0.0.1:9/deactivate".to_owned(),
                api_token: None,
            })
            .expect("client"),
        );
        let (svc, sk) = build_service(1_750_000_000_000, provider);
        let token = sign_payload(&sk, &lifetime_payload());
        svc.activate_offline(&token).expect("activate");
        assert!(svc.status().expect("licensed").licensed);

        let after = svc.deactivate().await.expect("deactivate offline succeeds");
        assert!(!after.licensed);
        assert_eq!(after.state, LicenseStateKind::Unlicensed);
        assert_eq!(
            svc.keychain
                .get_secret(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
                .expect("keychain check"),
            None
        );
    }

    #[tokio::test]
    async fn bug_034_offline_license_handles_bom_and_keychain_error_never_leaks_secret() {
        struct LockedKeychain;
        impl KeychainBackend for LockedKeychain {
            fn get_secret(&self, _service: &str, _account: &str) -> AppResult<Option<String>> {
                Err(AppError::Internal(
                    "macOS Keychain is locked or denied access while trying to read the license."
                        .to_owned(),
                ))
            }
            fn set_secret(&self, _service: &str, _account: &str, _secret: &str) -> AppResult<()> {
                Err(AppError::Internal(
                    "macOS Keychain is locked or denied access while trying to save the license."
                        .to_owned(),
                ))
            }
            fn delete_secret(&self, _service: &str, _account: &str) -> AppResult<bool> {
                Err(AppError::Internal(
                    "macOS Keychain is locked or denied access while trying to delete the license."
                        .to_owned(),
                ))
            }
        }

        let (svc, sk) = build_service(1_750_000_000_000, Arc::new(UnconfiguredOnlineProvider));
        let token = sign_payload(&sk, &lifetime_payload());

        // 1. Offline key pasted with UTF-8 BOM + whitespace must activate seamlessly via `activate`.
        let bom_token = format!("\u{feff}  {token}\n  ");
        let st_tok = svc
            .activate(&bom_token)
            .await
            .expect("UTF-8 BOM before offline FF1 token must activate");
        assert!(st_tok.licensed);

        // 2. `.fflicense` JSON file saved with UTF-8 BOM must activate cleanly via `activate_offline_file`.
        let file_json = sign_file_json(&sk, &lifetime_payload());
        let bom_json = format!("\u{feff}\n{file_json}");
        let dir = tempfile::tempdir().expect("tempdir");
        let fflicense = dir.path().join("with-bom.fflicense");
        crate::test_support::write_file(&fflicense, bom_json.as_bytes());
        let st_file = svc
            .activate_offline_file(&fflicense)
            .expect("UTF-8 BOM in .fflicense file must activate");
        assert!(st_file.licensed);

        // 3. Keychain save failure propagates an actionable error without leaking the raw token (`NFR-013`).
        let mut parts = token
            .strip_prefix(OFFLINE_KEY_PREFIX)
            .expect("prefix")
            .split('.');
        let payload_b64 = parts.next().expect("payload");
        let signature_b64 = parts.next().expect("sig");
        let locked_svc = LicenseService::with_verifying_key(
            Arc::new(LockedKeychain),
            Arc::new(UnconfiguredOnlineProvider),
            Arc::new(FixedClock::utc(1_750_000_000_000)),
            sk.verifying_key(),
        );
        let err = locked_svc
            .activate_offline(&token)
            .expect_err("locked keychain must fail activation");
        let msg = err.to_string();
        assert!(msg.contains("Keychain is locked"), "unexpected msg: {msg}");
        assert!(
            !msg.contains(payload_b64),
            "error must never leak raw license payload"
        );
        assert!(
            !msg.contains(signature_b64),
            "error must never leak raw signature"
        );
    }

    trait OkTuple {
        fn pipe_ok(self) -> (&'static str, Self);
    }
    impl OkTuple for String {
        fn pipe_ok(self) -> (&'static str, Self) {
            ("200 OK", self)
        }
    }
}
