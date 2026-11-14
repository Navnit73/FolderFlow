//! What FolderFlow never processes (FR-016, SAFETY_MODEL "Ignore list").
//!
//! System files, incomplete downloads and FolderFlow's own staging files are
//! ignored unconditionally — a partial download must never be organized
//! (FR-015/016). Hidden and temporary files follow the user's settings.

use std::path::{Path, PathBuf};

use crate::database::repo_exclusions::{ExclusionEntry, ExclusionKind};
use crate::engine::paths::{nfc, nfc_str};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    SystemFile,
    PartialDownload,
    Temporary,
    Hidden,
    CacheDirectory,
    ExcludedFolder,
    ExcludedGlob,
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
    /// Absolute paths of excluded subfolders or files (pruned with everything inside).
    pub excluded_dirs: Vec<PathBuf>,
    /// Compiled filename glob exclusions from `exclusions` table.
    pub excluded_globs: Vec<glob::Pattern>,
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
    /// Builds [`IgnoreRules`] from global monitoring settings, per-folder settings,
    /// and any extra excluded directory paths.
    pub fn from_folder_and_monitoring(
        monitoring: &crate::settings::MonitoringSettings,
        folder: &crate::folders::FolderSettings,
        extra_excluded_dirs: Vec<PathBuf>,
    ) -> Self {
        let mut excluded_dirs: Vec<PathBuf> = folder
            .excluded_subfolders
            .iter()
            .map(|s| nfc(Path::new(s)))
            .collect();
        for d in extra_excluded_dirs {
            let clean = nfc(&d);
            if !excluded_dirs.contains(&clean) {
                excluded_dirs.push(clean);
            }
        }
        Self {
            ignore_hidden: monitoring.ignore_hidden_files,
            ignore_temporary: monitoring.ignore_temporary_files,
            include_extensions: folder
                .include_extensions
                .iter()
                .map(|e| e.trim().trim_start_matches('.').to_lowercase())
                .collect(),
            exclude_extensions: folder
                .exclude_extensions
                .iter()
                .map(|e| e.trim().trim_start_matches('.').to_lowercase())
                .collect(),
            excluded_dirs,
            excluded_globs: Vec::new(),
        }
    }

    /// Merges database [`ExclusionEntry`] records (global + per-folder) into these ignore rules (`BUG-026`).
    pub fn apply_db_exclusions(&mut self, entries: &[ExclusionEntry]) {
        for entry in entries {
            match entry.exclusion_type {
                ExclusionKind::Path => {
                    let clean = nfc(Path::new(&entry.path));
                    if !self.excluded_dirs.contains(&clean) {
                        self.excluded_dirs.push(clean);
                    }
                }
                ExclusionKind::Extension => {
                    let ext = entry.path.trim().trim_start_matches('.').to_lowercase();
                    if !ext.is_empty() && !self.exclude_extensions.contains(&ext) {
                        self.exclude_extensions.push(ext);
                    }
                }
                ExclusionKind::Glob => {
                    if let Ok(pat) = glob::Pattern::new(&nfc_str(&entry.path)) {
                        self.excluded_globs.push(pat);
                    }
                }
            }
        }
    }

    /// Checks ancestor directories between `folder_root` and `entry_path` plus the entry itself.
    pub fn is_path_excluded(
        &self,
        folder_root: &Path,
        entry_path: &Path,
        name: &str,
        ext: Option<&str>,
        hidden: bool,
        is_dir_or_pkg: bool,
    ) -> bool {
        let root_nfc = nfc(folder_root);
        let entry_nfc = nfc(entry_path);
        if let Ok(rel) = entry_nfc.strip_prefix(&root_nfc) {
            let mut cur = root_nfc;
            if let Some(parent) = rel.parent() {
                for comp in parent.components() {
                    cur.push(comp);
                    let comp_name = comp.as_os_str().to_string_lossy();
                    if self
                        .check_dir(&cur, &comp_name, comp_name.starts_with('.'))
                        .is_some()
                    {
                        return true;
                    }
                }
            }
        }
        if is_dir_or_pkg {
            self.check_dir(&entry_nfc, name, hidden).is_some()
                || self.check_file(&entry_nfc, name, ext, hidden).is_some()
        } else {
            self.check_file(&entry_nfc, name, ext, hidden).is_some()
        }
    }

    /// For a file (or package) entry. `None` = process it.
    pub fn check_file(
        &self,
        path: &Path,
        name: &str,
        ext: Option<&str>,
        hidden: bool,
    ) -> Option<SkipReason> {
        let name_nfc = nfc_str(name);
        if name_nfc.starts_with(STAGING_PREFIX) {
            return Some(SkipReason::OwnStagingFile);
        }
        if SYSTEM_NAMES.contains(&name)
            || SYSTEM_NAMES.contains(&name_nfc.as_str())
            || name.starts_with("._")
        {
            return Some(SkipReason::SystemFile);
        }
        if ext.is_some_and(|e| PARTIAL_EXTENSIONS.contains(&e)) {
            return Some(SkipReason::PartialDownload);
        }
        let path_nfc = nfc(path);
        if self
            .excluded_dirs
            .iter()
            .any(|d| path_nfc == *d || path_nfc.starts_with(d))
        {
            return Some(SkipReason::ExcludedFolder);
        }
        if self.excluded_globs.iter().any(|g| g.matches(&name_nfc)) {
            return Some(SkipReason::ExcludedGlob);
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
        let ext_lower = ext.map(str::to_lowercase);
        if ext_lower
            .as_deref()
            .is_some_and(|e| self.exclude_extensions.iter().any(|x| x == e))
        {
            return Some(SkipReason::ExcludedExtension);
        }
        if !self.include_extensions.is_empty()
            && !ext_lower
                .as_deref()
                .is_some_and(|e| self.include_extensions.iter().any(|x| x == e))
        {
            return Some(SkipReason::NotIncludedExtension);
        }
        None
    }

    /// For a directory entry. `Some` = do not descend.
    pub fn check_dir(&self, path: &Path, name: &str, hidden: bool) -> Option<SkipReason> {
        let name_nfc = nfc_str(name);
        if name_nfc.starts_with(STAGING_PREFIX) {
            return Some(SkipReason::OwnStagingFile);
        }
        let ext = crate::engine::classifier::extension_of(name);
        if ext
            .as_deref()
            .is_some_and(|e| PARTIAL_EXTENSIONS.contains(&e))
        {
            return Some(SkipReason::PartialDownload);
        }
        if CACHE_DIRS.contains(&name) || CACHE_DIRS.contains(&name_nfc.as_str()) {
            return Some(SkipReason::CacheDirectory);
        }
        let path_nfc = nfc(path);
        if self.excluded_dirs.iter().any(|d| path_nfc.starts_with(d)) {
            return Some(SkipReason::ExcludedFolder);
        }
        if self.excluded_globs.iter().any(|g| g.matches(&name_nfc)) {
            return Some(SkipReason::ExcludedGlob);
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
