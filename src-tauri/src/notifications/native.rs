//! Native macOS notification delivery via `UNUserNotificationCenter` (`FR-200`, `P7-T04`).
//!
//! Automatically detects unbundled execution (`cargo test` / CLI) via `NSBundle.bundleIdentifier`
//! before touching `UNUserNotificationCenter::currentNotificationCenter()`, avoiding Apple's
//! unbundled `bundleProxyForCurrentProcess` NSException.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNNotificationSound, UNUserNotificationCenter,
};

use crate::error::AppResult;
use crate::notifications::{NotificationRecord, NotificationSink};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDeliveryOutcome {
    Sent,
    SkippedUnbundled,
}

/// Abstract backend for dispatching native macOS Notification Center alerts (`R-NOTIF-05`).
pub trait NativeNotificationBackend: Send + Sync {
    fn is_available(&self) -> bool;
    fn deliver(&self, record: &NotificationRecord) -> AppResult<NativeDeliveryOutcome>;
}

/// Live `UNUserNotificationCenter` backend for macOS `.app` bundles.
#[derive(Debug, Default)]
pub struct MacOsNativeNotifier {
    auth_requested: AtomicBool,
}

impl MacOsNativeNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    fn has_bundle_identifier() -> bool {
        NSBundle::mainBundle().bundleIdentifier().is_some()
    }
}

impl NativeNotificationBackend for MacOsNativeNotifier {
    fn is_available(&self) -> bool {
        Self::has_bundle_identifier()
    }

    fn deliver(&self, record: &NotificationRecord) -> AppResult<NativeDeliveryOutcome> {
        if !Self::has_bundle_identifier() {
            return Ok(NativeDeliveryOutcome::SkippedUnbundled);
        }

        let center = UNUserNotificationCenter::currentNotificationCenter();
        if !self.auth_requested.swap(true, Ordering::SeqCst) {
            let options = UNAuthorizationOptions::Alert
                | UNAuthorizationOptions::Sound
                | UNAuthorizationOptions::Badge;
            let auth_handler = RcBlock::new(|granted: Bool, err: *mut NSError| {
                if !err.is_null() {
                    tracing::warn!(
                        target: "folderflow::notifications",
                        "native notification authorization reported an NSError"
                    );
                } else {
                    tracing::debug!(
                        target: "folderflow::notifications",
                        granted = granted.as_bool(),
                        "native notification authorization resolved"
                    );
                }
            });
            center.requestAuthorizationWithOptions_completionHandler(options, &auth_handler);
        }

        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&record.title));
        content.setBody(&NSString::from_str(&record.body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));

        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&record.id),
            &content,
            None,
        );
        let add_handler = RcBlock::new(|err: *mut NSError| {
            if !err.is_null() {
                tracing::debug!(
                    target: "folderflow::notifications",
                    "failed to schedule native user notification"
                );
            }
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&add_handler));
        Ok(NativeDeliveryOutcome::Sent)
    }
}

/// Forwards delivered `NotificationService` records to both an app sink (WebView event)
/// and a native macOS Notification Center dispatcher (`R-NOTIF-05`).
pub struct CompositeNotificationSink {
    pub ui_sink: Arc<dyn NotificationSink>,
    pub native: Arc<dyn NativeNotificationBackend>,
}

impl NotificationSink for CompositeNotificationSink {
    fn emit(&self, record: &NotificationRecord) {
        self.ui_sink.emit(record);
        if let Err(err) = self.native.deliver(record) {
            tracing::warn!(
                target: "folderflow::notifications",
                error = %err,
                "native notification delivery failed"
            );
        }
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct RecordingNativeNotifier {
    delivered: Mutex<Vec<NotificationRecord>>,
}

#[cfg(test)]
impl RecordingNativeNotifier {
    pub fn snapshot(&self) -> Vec<NotificationRecord> {
        self.delivered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

#[cfg(test)]
impl NativeNotificationBackend for RecordingNativeNotifier {
    fn is_available(&self) -> bool {
        true
    }

    fn deliver(&self, record: &NotificationRecord) -> AppResult<NativeDeliveryOutcome> {
        self.delivered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(record.clone());
        Ok(NativeDeliveryOutcome::Sent)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, Ordering};

    use super::*;
    use crate::notifications::{
        NotificationConfig, NotificationInput, NotificationKind, NotificationService,
        RecordingNotificationSink,
    };
    use crate::settings::{NotificationEventPrefs, NotificationSettings, Settings};
    use crate::utils::clock::Clock;

    #[derive(Debug)]
    struct StepClock(AtomicI64);
    impl Clock for StepClock {
        fn now_ms(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
        fn utc_offset_secs(&self, _epoch_ms: i64) -> i32 {
            0
        }
    }

    #[test]
    fn r_notif_05_native_dispatcher_receives_batched_throttled_notifications_and_skips_unbundled() {
        let ui_sink = Arc::new(RecordingNotificationSink::default());
        let native = Arc::new(RecordingNativeNotifier::default());
        let composite = Arc::new(CompositeNotificationSink {
            ui_sink: ui_sink.clone(),
            native: native.clone(),
        });

        let app_settings = Settings {
            notifications: NotificationSettings {
                enabled: true,
                batch_window_secs: 5,
                cooldown_secs: 30,
                large_batch_threshold: 10,
                per_type: NotificationEventPrefs::default(),
            },
            ..Settings::default()
        };

        let clock = Arc::new(StepClock(AtomicI64::new(10_000)));
        let svc = NotificationService::new(
            clock.clone(),
            composite,
            NotificationConfig::from_app_settings(&app_settings),
        );

        // Critical alert is batched/throttled according to settings, then delivered to BOTH
        // the frontend UI sink and the native macOS Notification Center backend on flush.
        svc.notify(NotificationInput::destination_unavailable(
            Some("f1".into()),
            "/Volumes/ExternalSSD is disconnected.",
            None,
        ));
        assert_eq!(ui_sink.records().len(), 0);
        clock.0.fetch_add(6_000, Ordering::SeqCst);
        let first_flushed = svc.flush_due();
        assert_eq!(first_flushed.len(), 1);
        assert_eq!(ui_sink.records().len(), 1);
        let first_native = native.snapshot();
        assert_eq!(first_native.len(), 1);
        assert_eq!(first_native[0].kind, NotificationKind::DestinationUnavailable);

        // Two rapid success notifications batch into 1 native notification (never 1 per file).
        svc.notify(NotificationInput::success_summary(Some("f1".into()), 2));
        svc.notify(NotificationInput::success_summary(Some("f1".into()), 3));
        assert_eq!(native.snapshot().len(), 1);

        clock.0.fetch_add(6_000, Ordering::SeqCst);
        let flushed = svc.flush_due();
        assert_eq!(flushed.len(), 1);
        let all_native = native.snapshot();
        assert_eq!(all_native.len(), 2);
        assert_eq!(all_native[1].kind, NotificationKind::SuccessSummary);
        assert_eq!(all_native[1].count, 5);
        assert_eq!(all_native[1].batch_count, 2);

        // Real MacOsNativeNotifier skips cleanly outside a `.app` bundle instead of raising ObjC exception.
        let real = MacOsNativeNotifier::new();
        assert!(!real.is_available());
        assert_eq!(
            real.deliver(&all_native[0]).expect("unbundled safe"),
            NativeDeliveryOutcome::SkippedUnbundled
        );
    }
}
