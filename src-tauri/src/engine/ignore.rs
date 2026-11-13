//! What FolderFlow never processes (FR-016, SAFETY_MODEL "Ignore list").
//!
//! System files, incomplete downloads and FolderFlow's own staging files are
//! ignored unconditionally — a partial download must never be organized
//! (FR-015/016). Hidden and temporary files follow the user's settings.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    SystemFile,
    PartialDownload,
    Temporary,
    Hidden,
    CacheDirectory,
    ExcludedFolder,
    ExcludedExtension,
    NotIncludedExtension,
    Symlink,
    Special,
    OwnStagingFile,
}

#[derive(Debug, Clone, Default)]
pub struct IgnoreRules {
    pub ignore_hidden: bool,
    pub ignore_temporary: bool,
    /// If non-empty, only these extensions are processed.
    pub include_extensions: Vec<String>,
    pub exclude_extensions: Vec<String>,
    /// Absolute paths of excluded subfolders (pruned with everything inside).
    pub excluded_dirs: Vec<PathBuf>,
}

/// Names that are always system metadata.
const SYSTEM_NAMES: &[&str] = &[
    ".DS_Store",
    ".localized",
    "Icon\r",
    ".com.apple.timemachine.donotpresent",
];
/// Extensions of in-progress downloads (Safari, Chrome, Firefox, Opera, qBittorrent, generic).
const PARTIAL_EXTENSIONS: &[&str] = &[
    "download",
    "crdownload",
    "part",
    "partial",
    "opdownload",
    "!qb",
];
const TEMP_EXTENSIONS: &[&str] = &["tmp", "temp", "swp", "swo"];
/// Directories never descended into (VCS, dependency and cache folders, macOS volume metadata).
const CACHE_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    ".cache",
    "__pycache__",
    ".venv",
    ".Trash",
    ".Trashes",
    ".Spotlight-V100",
    ".fseventsd",
    ".TemporaryItems",
    ".DocumentRevisions-V100",
];
/// Prefix of FolderFlow's own staging files (SAFETY_MODEL cross-volume move).
pub const STAGING_PREFIX: &str = ".folderflow-";

impl IgnoreRules {
    /// For a file (or package) entry. `None` = process it.
    pub fn check_file(
        &self,
        _path: &Path,
        name: &str,
        ext: Option<&str>,
        hidden: bool,
    ) -> Option<SkipReason> {
        if name.starts_with(STAGING_PREFIX) {
            return Some(SkipReason::OwnStagingFile);
        }
        if SYSTEM_NAMES.contains(&name) || name.starts_with("._") {
            return Some(SkipReason::SystemFile);
        }
        if ext.is_some_and(|e| PARTIAL_EXTENSIONS.contains(&e)) {
            return Some(SkipReason::PartialDownload);
        }
        if hidden && self.ignore_hidden {
            return Some(SkipReason::Hidden);
        }
        if self.ignore_temporary
            && (ext.is_some_and(|e| TEMP_EXTENSIONS.contains(&e))
                || name.starts_with("~$")
                || name.ends_with('~'))
        {
            return Some(SkipReason::Temporary);
        }
        if ext.is_some_and(|e| self.exclude_extensions.iter().any(|x| x == e)) {
            return Some(SkipReason::ExcludedExtension);
        }
        if !self.include_extensions.is_empty()
            && !ext.is_some_and(|e| self.include_extensions.iter().any(|x| x == e))
        {
            return Some(SkipReason::NotIncludedExtension);
        }
        None
    }

    /// For a directory entry. `Some` = do not descend.
    pub fn check_dir(&self, path: &Path, name: &str, hidden: bool) -> Option<SkipReason> {
        if name.starts_with(STAGING_PREFIX) {
            return Some(SkipReason::OwnStagingFile);
        }
        let ext = crate::engine::classifier::extension_of(name);
        if ext
            .as_deref()
            .is_some_and(|e| PARTIAL_EXTENSIONS.contains(&e))
        {
            return Some(SkipReason::PartialDownload);
        }
        if CACHE_DIRS.contains(&name) {
            return Some(SkipReason::CacheDirectory);
        }
        if self.excluded_dirs.iter().any(|d| path.starts_with(d)) {
            return Some(SkipReason::ExcludedFolder);
        }
        if hidden && self.ignore_hidden {
            return Some(SkipReason::Hidden);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> IgnoreRules {
        IgnoreRules {
            ignore_hidden: true,
            ignore_temporary: true,
            ..Default::default()
        }
    }
    fn file(r: &IgnoreRules, name: &str, hidden: bool) -> Option<SkipReason> {
        let ext = crate::engine::classifier::extension_of(name);
        r.check_file(&Path::new("/root").join(name), name, ext.as_deref(), hidden)
    }

    #[test]
    fn system_files_and_partial_downloads_are_always_ignored() {
        let lenient = IgnoreRules::default(); // hidden + temp NOT ignored
        for name in [".DS_Store", "._photo.jpg", ".localized", "Icon\r"] {
            assert_eq!(
                file(&lenient, name, true),
                Some(SkipReason::SystemFile),
                "{name}"
            );
        }
        for name in [
            "movie.mp4.crdownload",
            "a.zip.download",
            "b.part",
            "c.partial",
            "d.opdownload",
            "e.!qB",
        ] {
            assert_eq!(
                file(&lenient, name, false),
                Some(SkipReason::PartialDownload),
                "{name}"
            );
        }
        assert_eq!(
            file(&lenient, ".folderflow-0190.partial", true),
            Some(SkipReason::OwnStagingFile)
        );
        assert_eq!(
            lenient.check_dir(Path::new("/root/x.zip.download"), "x.zip.download", false),
            Some(SkipReason::PartialDownload)
        );
    }

    #[test]
    fn hidden_and_temporary_follow_settings() {
        assert_eq!(file(&rules(), ".env", true), Some(SkipReason::Hidden));
        assert_eq!(file(&IgnoreRules::default(), ".env", true), None);
        for name in ["x.tmp", "~$report.docx", "notes.txt~", "a.swp"] {
            assert_eq!(
                file(&rules(), name, false),
                Some(SkipReason::Temporary),
                "{name}"
            );
            assert_eq!(file(&IgnoreRules::default(), name, false), None, "{name}");
        }
    }

    #[test]
    fn cache_and_excluded_directories_are_pruned() {
        let mut r = rules();
        r.excluded_dirs = vec![PathBuf::from("/root/Keep")];
        for name in ["node_modules", ".git", "__pycache__", ".Trash", ".cache"] {
            assert!(
                r.check_dir(&Path::new("/root").join(name), name, name.starts_with('.'))
                    .is_some(),
                "{name}"
            );
        }
        assert_eq!(
            r.check_dir(Path::new("/root/Keep"), "Keep", false),
            Some(SkipReason::ExcludedFolder)
        );
        assert_eq!(
            r.check_dir(Path::new("/root/Keep/sub"), "sub", false),
            Some(SkipReason::ExcludedFolder)
        );
        assert_eq!(
            r.check_dir(Path::new("/root/Keeper"), "Keeper", false),
            None
        );
        assert_eq!(
            r.check_dir(Path::new("/root/Projects"), "Projects", false),
            None
        );
    }

    #[test]
    fn include_and_exclude_extensions() {
        let mut r = rules();
        r.exclude_extensions = vec!["dmg".into()];
        assert_eq!(
            file(&r, "Setup.DMG", false),
            Some(SkipReason::ExcludedExtension)
        );
        assert_eq!(file(&r, "a.pdf", false), None);
        r.include_extensions = vec!["pdf".into(), "png".into()];
        assert_eq!(file(&r, "a.pdf", false), None);
        assert_eq!(
            file(&r, "a.zip", false),
            Some(SkipReason::NotIncludedExtension)
        );
        assert_eq!(
            file(&r, "noext", false),
            Some(SkipReason::NotIncludedExtension)
        );
    }
}
