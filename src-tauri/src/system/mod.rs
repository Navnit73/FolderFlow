//! macOS integration (no shell, no `Command` — SAFETY_MODEL S11).

use std::path::Path;

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

/// Reveals a verified path in Finder via `NSWorkspace::selectFile_inFileViewerRootedAtPath`.
/// Never spawns a shell or external process (`ADR-011`, `SAFETY_MODEL.md` S11).
pub fn reveal_in_finder(path: &Path) -> bool {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;
    let Some(path_str) = path.to_str() else {
        return false;
    };
    let ns_path = NSString::from_str(path_str);
    let empty_root = NSString::from_str("");
    NSWorkspace::sharedWorkspace().selectFile_inFileViewerRootedAtPath(Some(&ns_path), &empty_root)
}
