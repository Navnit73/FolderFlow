//! macOS Launch at Login via `SMAppService` (`FR-180`, `P7-T02`, macOS 13+).

use std::sync::Arc;

use objc2_foundation::NSBundle;
use serde::{Deserialize, Serialize};
use smappservice_rs::{AppService, ServiceManagementError, ServiceStatus, ServiceType};
use specta::Type;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LoginItemStatus {
    Enabled,
    RequiresApproval,
    #[default]
    NotRegistered,
    NotFound,
    Unsupported,
}

pub trait LoginItemBackend: Send + Sync {
    fn status(&self) -> AppResult<LoginItemStatus>;
    fn set_enabled(&self, enabled: bool) -> AppResult<LoginItemStatus>;
    fn open_login_items_settings(&self) -> bool;
}

/// Real `SMAppService::mainAppService()` backend (`SMAppService` on macOS 13+).
#[derive(Debug, Default)]
pub struct SmAppServiceBackend;

impl SmAppServiceBackend {
    pub fn new() -> Self {
        Self
    }

    fn is_bundled() -> bool {
        NSBundle::mainBundle().bundleIdentifier().is_some()
    }

    fn map_status(status: ServiceStatus) -> LoginItemStatus {
        match status {
            ServiceStatus::Enabled => LoginItemStatus::Enabled,
            ServiceStatus::RequiresApproval => LoginItemStatus::RequiresApproval,
            ServiceStatus::NotRegistered => LoginItemStatus::NotRegistered,
            ServiceStatus::NotFound => LoginItemStatus::NotFound,
        }
    }

    fn map_error(err: ServiceManagementError) -> AppError {
        let msg = match err {
            ServiceManagementError::LaunchDeniedByUser => {
                "Launch at login was denied in System Settings → General → Login Items."
                    .to_owned()
            }
            ServiceManagementError::InvalidSignature => {
                "Launch at login requires a signed FolderFlow.app bundle.".to_owned()
            }
            ServiceManagementError::JobNotFound | ServiceManagementError::ServiceUnavailable => {
                "Login item service is unavailable outside the packaged FolderFlow.app bundle."
                    .to_owned()
            }
            other => format!("Could not update Launch at login: {other}"),
        };
        AppError::validation("general.launchAtLogin", msg)
    }
}

impl LoginItemBackend for SmAppServiceBackend {
    fn status(&self) -> AppResult<LoginItemStatus> {
        if !Self::is_bundled() {
            return Ok(LoginItemStatus::NotFound);
        }
        let service = AppService::new(ServiceType::MainApp);
        Ok(Self::map_status(service.status()))
    }

    fn set_enabled(&self, enabled: bool) -> AppResult<LoginItemStatus> {
        if !Self::is_bundled() {
            if !enabled {
                return Ok(LoginItemStatus::NotFound);
            }
            return Err(AppError::validation(
                "general.launchAtLogin",
                "Launch at login can only be registered from the packaged FolderFlow.app bundle.",
            ));
        }

        let service = AppService::new(ServiceType::MainApp);
        if enabled {
            service.register().map_err(Self::map_error)?;
        } else {
            match service.unregister() {
                Ok(()) => {}
                Err(ServiceManagementError::JobNotFound) => {}
                Err(err) => return Err(Self::map_error(err)),
            }
        }
        Ok(Self::map_status(service.status()))
    }

    fn open_login_items_settings(&self) -> bool {
        AppService::open_system_settings_login_items();
        true
    }
}

#[derive(Clone)]
pub struct LoginItemService {
    backend: Arc<dyn LoginItemBackend>,
}

impl LoginItemService {
    pub fn new(backend: Arc<dyn LoginItemBackend>) -> Self {
        Self { backend }
    }

    pub fn production() -> Self {
        Self::new(Arc::new(SmAppServiceBackend::new()))
    }

    pub fn status(&self) -> AppResult<LoginItemStatus> {
        self.backend.status()
    }

    pub fn set_enabled(&self, enabled: bool) -> AppResult<LoginItemStatus> {
        self.backend.set_enabled(enabled)
    }

    pub fn open_login_items_settings(&self) -> bool {
        self.backend.open_login_items_settings()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct MockLoginItemBackend {
        state: Mutex<LoginItemStatus>,
        fail_next: Mutex<Option<String>>,
    }

    impl LoginItemBackend for MockLoginItemBackend {
        fn status(&self) -> AppResult<LoginItemStatus> {
            Ok(*self.state.lock().unwrap_or_else(|e| e.into_inner()))
        }

        fn set_enabled(&self, enabled: bool) -> AppResult<LoginItemStatus> {
            if let Some(msg) = self
                .fail_next
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                return Err(AppError::validation("general.launchAtLogin", msg));
            }
            let next = if enabled {
                LoginItemStatus::Enabled
            } else {
                LoginItemStatus::NotRegistered
            };
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = next;
            Ok(next)
        }

        fn open_login_items_settings(&self) -> bool {
            true
        }
    }

    #[test]
    fn r_login_01_sync_toggles_smappservice_and_surfaces_errors() {
        let backend = Arc::new(MockLoginItemBackend::default());
        let svc = LoginItemService::new(backend.clone());

        assert_eq!(svc.status().expect("not reg"), LoginItemStatus::NotRegistered);

        assert_eq!(
            svc.set_enabled(true).expect("enable"),
            LoginItemStatus::Enabled
        );
        assert_eq!(
            svc.set_enabled(false).expect("disable"),
            LoginItemStatus::NotRegistered
        );

        *backend.fail_next.lock().unwrap_or_else(|e| e.into_inner()) =
            Some("Denied by System Settings".into());
        assert!(svc.set_enabled(true).is_err());

        // Real macOS SMAppService backend never panics in unbundled cargo test processes.
        let real = SmAppServiceBackend::new();
        let st = real.status().expect("unbundled status");
        assert_eq!(st, LoginItemStatus::NotFound);
        assert_eq!(
            real.set_enabled(false).expect("unbundled disable idempotent"),
            LoginItemStatus::NotFound
        );
    }
}
