//! macOS integration (no shell, no `Command` — SAFETY_MODEL S11).

/// System Settings → Privacy & Security → Files and Folders.
pub const PRIVACY_FILES_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders";

/// Opens the Files and Folders privacy pane. Fixed URL only; never user input.
pub fn open_privacy_settings() -> bool {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    let Some(url) = NSURL::URLWithString(&NSString::from_str(PRIVACY_FILES_URL)) else {
        return false;
    };
    NSWorkspace::sharedWorkspace().openURL(&url)
}
