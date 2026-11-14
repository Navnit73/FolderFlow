//! Application notification service: per-type preferences, time-window batching,
//! cooldown throttling, path redaction, and sink delivery (FR-200..202, P6-T06).

pub mod native;
pub mod service;

pub use native::{
    CompositeNotificationSink, MacOsNativeNotifier, NativeDeliveryOutcome,
    NativeNotificationBackend,
};
pub use service::{
    NoopNotificationSink, NotificationConfig, NotificationInput, NotificationKind,
    NotificationRecord, NotificationService, NotificationSeverity, NotificationSink, NotifyResult,
    RecordingNotificationSink, MAX_NOTIFICATION_HISTORY,
};
