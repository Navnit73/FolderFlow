//! Minimal macOS `NSWorkspace` observer bridge for FolderFlow (Phase 7, P7-T03).
//!
//! Isolated into a tiny helper crate so the main `folderflow` crate keeps
//! `#![deny(unsafe_code)]` while cleanly unregistering notification tokens on drop.

#![deny(unsafe_op_in_unsafe_fn)]

use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceDidMountNotification, NSWorkspaceDidUnmountNotification,
    NSWorkspaceDidWakeNotification, NSWorkspaceVolumeURLKey, NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNotificationName, NSObjectProtocol, NSURL,
};

/// System-level power & volume lifecycle events observed via `NSWorkspace`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceSystemEvent {
    WillSleep,
    DidWake,
    VolumeMounted { mount_point: PathBuf },
    VolumeUnmounted { mount_point: PathBuf },
}

/// Keeps `NSWorkspace` notification observers active until dropped.
pub struct WorkspaceObserverGuard {
    center: Retained<NSNotificationCenter>,
    tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl std::fmt::Debug for WorkspaceObserverGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceObserverGuard")
            .field("observer_count", &self.tokens.len())
            .finish()
    }
}

impl Drop for WorkspaceObserverGuard {
    fn drop(&mut self) {
        for token in self.tokens.drain(..) {
            let obj: &AnyObject = token.as_ref();
            // SAFETY: Each token was created by `addObserverForName_object_queue_usingBlock`
            // on `self.center`, remains retained until drained here, and is removed at most once.
            unsafe {
                self.center.removeObserver(obj);
            }
        }
    }
}

fn extract_volume_path(notif: &NSNotification) -> Option<PathBuf> {
    let info = notif.userInfo()?;
    // SAFETY: `NSWorkspaceVolumeURLKey` is a constant AppKit `extern static` NSString key.
    let key_obj: &AnyObject = unsafe { NSWorkspaceVolumeURLKey }.as_ref();
    let val = info.objectForKey(key_obj)?;
    let url = val.downcast::<NSURL>().ok()?;
    let path_str = url.path()?;
    Some(PathBuf::from(path_str.to_string()))
}

fn register_observer<F>(
    center: &NSNotificationCenter,
    name: &NSNotificationName,
    handler: F,
) -> Retained<ProtocolObject<dyn NSObjectProtocol>>
where
    F: Fn(&NSNotification) + 'static,
{
    let block = RcBlock::new(move |notif_ptr: NonNull<NSNotification>| {
        // SAFETY: `NSNotificationCenter` supplies a valid non-null `NSNotification` pointer
        // for the synchronous duration of the observer callback.
        let notif = unsafe { notif_ptr.as_ref() };
        handler(notif);
    });
    // SAFETY: `name` is a valid static Cocoa `NSNotificationName`, `object` and `queue` are `None`
    // (deliver on posting thread without lifetime coupling), and `RcBlock` keeps the closure valid
    // until the returned token is unregistered via `removeObserver:` on drop.
    unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) }
}

/// Starts observing macOS sleep/wake and volume mount/unmount notifications.
pub fn start_workspace_observers<F>(callback: F) -> WorkspaceObserverGuard
where
    F: Fn(WorkspaceSystemEvent) + Send + Sync + 'static,
{
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let cb = Arc::new(callback);

    let tokens = vec![
        {
            let cb = Arc::clone(&cb);
            // SAFETY: `NSWorkspaceWillSleepNotification` is a constant Foundation static name.
            let name = unsafe { NSWorkspaceWillSleepNotification };
            register_observer(&center, name, move |_| {
                cb(WorkspaceSystemEvent::WillSleep);
            })
        },
        {
            let cb = Arc::clone(&cb);
            // SAFETY: `NSWorkspaceDidWakeNotification` is a constant Foundation static name.
            let name = unsafe { NSWorkspaceDidWakeNotification };
            register_observer(&center, name, move |_| {
                cb(WorkspaceSystemEvent::DidWake);
            })
        },
        {
            let cb = Arc::clone(&cb);
            // SAFETY: `NSWorkspaceDidMountNotification` is a constant Foundation static name.
            let name = unsafe { NSWorkspaceDidMountNotification };
            register_observer(&center, name, move |notif| {
                if let Some(mount_point) = extract_volume_path(notif) {
                    cb(WorkspaceSystemEvent::VolumeMounted { mount_point });
                }
            })
        },
        {
            let cb = Arc::clone(&cb);
            // SAFETY: `NSWorkspaceDidUnmountNotification` is a constant Foundation static name.
            let name = unsafe { NSWorkspaceDidUnmountNotification };
            register_observer(&center, name, move |notif| {
                if let Some(mount_point) = extract_volume_path(notif) {
                    cb(WorkspaceSystemEvent::VolumeUnmounted { mount_point });
                }
            })
        },
    ];

    WorkspaceObserverGuard { center, tokens }
}
