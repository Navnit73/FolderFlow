//! Path authority (SAFETY_MODEL S7, S8; FR-072, FR-073).
//!
//! Every path FolderFlow acts on must be authorised here first:
//! 1. lexical checks — absolute, no `.`/`..`, valid component names, length limits;
//! 2. resolution — the deepest *existing* ancestor of the parent is canonicalised
//!    (so symlinked directories cannot smuggle a path outside a root); the
//!    remaining components and the leaf are appended unresolved;
//! 3. containment — the result must lie inside an approved root (component-wise,
//!    Unicode-NFC compared, since APFS/HFS+ are normalisation-insensitive).
//!
//! The leaf is never followed: a symlink leaf is reported, not resolved.

use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

/// macOS NAME_MAX.
pub const MAX_COMPONENT_BYTES: usize = 255;
/// Conservative PATH_MAX for paths FolderFlow creates or acts on.
pub const MAX_PATH_BYTES: usize = 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("path must be absolute")]
    NotAbsolute,
    #[error("invalid path component: {0}")]
    InvalidComponent(String),
    #[error("path is too long")]
    TooLong,
    #[error("path is outside the folders FolderFlow is allowed to use")]
    OutsideApprovedRoots,
    #[error("this folder can't be used: {0}")]
    ForbiddenRoot(String),
    #[error("not a folder")]
    NotADirectory,
    #[error("path does not exist")]
    NotFound,
    #[error("permission denied")]
    PermissionDenied,
    #[error("i/o error: {0}")]
    Io(String),
}

impl From<std::io::Error> for PathError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            ErrorKind::NotFound => Self::NotFound,
            ErrorKind::PermissionDenied => Self::PermissionDenied,
            ErrorKind::NotADirectory => Self::NotADirectory,
            _ => Self::Io(e.kind().to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    Monitored,
    Destination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedRoot {
    pub path: PathBuf,
    pub kind: RootKind,
}

/// A path that passed authorisation. Parents are resolved; the leaf is not
/// (so a symlink leaf is acted on as a link, never followed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedPath {
    pub path: PathBuf,
    pub root: PathBuf,
    pub leaf_is_symlink: bool,
}

/// Context for deciding which folders may become roots.
#[derive(Debug, Clone)]
pub struct RootPolicy {
    pub home: PathBuf,
    pub app_data_dir: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct PathAuthority {
    roots: Vec<ApprovedRoot>,
}

impl PathAuthority {
    /// Roots are canonicalised (when they exist) and NFC-normalised once here.
    pub fn new(roots: Vec<ApprovedRoot>) -> Self {
        let roots = roots
            .into_iter()
            .map(|r| ApprovedRoot {
                path: nfc(&r.path.canonicalize().unwrap_or(r.path)),
                kind: r.kind,
            })
            .collect();
        Self { roots }
    }

    pub fn roots(&self) -> &[ApprovedRoot] {
        &self.roots
    }

    pub fn authorize(&self, path: &Path) -> Result<AuthorizedPath, PathError> {
        lexical_check(path)?;
        let (resolved, leaf_is_symlink) = resolve(path)?;
        let resolved = nfc(&resolved);
        let root = self
            .roots
            .iter()
            .filter(|r| resolved.starts_with(&r.path))
            .max_by_key(|r| r.path.components().count())
            .ok_or(PathError::OutsideApprovedRoots)?;
        Ok(AuthorizedPath {
            path: resolved,
            root: root.path.clone(),
            leaf_is_symlink,
        })
    }
}

/// Validates one file or folder name (no separators, not `.`/`..`, ≤ 255 bytes).
pub fn validate_component(name: &str) -> Result<(), PathError> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err(PathError::InvalidComponent(name.to_owned()));
    }
    if name.len() > MAX_COMPONENT_BYTES {
        return Err(PathError::InvalidComponent(
            "name is longer than 255 bytes".into(),
        ));
    }
    Ok(())
}

/// Checks a folder the user wants to monitor or use as a destination root.
/// Returns its canonical, NFC-normalised path.
pub fn validate_new_root(path: &Path, policy: &RootPolicy) -> Result<PathBuf, PathError> {
    lexical_check(path)?;
    let meta = std::fs::metadata(path)?;
    if !meta.is_dir() {
        return Err(PathError::NotADirectory);
    }
    let canon = nfc(&path.canonicalize()?);
    check_not_forbidden_root(&canon, policy)
}

/// Validates lexical safety and verifies `path` is not a protected macOS system folder,
/// user's home root, Library, or FolderFlow's own app-data directory. Works even if `path`
/// does not yet exist on disk.
pub fn check_not_forbidden_root(path: &Path, policy: &RootPolicy) -> Result<PathBuf, PathError> {
    lexical_check(path)?;
    let canon_or = |p: &Path| nfc(&p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
    let canon = canon_or(path);
    let home = canon_or(&policy.home);
    let app_data = canon_or(&policy.app_data_dir);

    const EXACT: &[&str] = &[
        "/",
        "/private",
        "/private/var",
        "/Users",
        "/Volumes",
        "/System/Volumes/Data",
    ];
    const SUBTREE: &[&str] = &[
        "/System",
        "/Library",
        "/Applications",
        "/usr",
        "/bin",
        "/sbin",
        "/opt",
        "/cores",
        "/dev",
        "/private/etc",
        "/etc",
    ];
    let forbid = |why: &str| Err(PathError::ForbiddenRoot(why.to_owned()));

    if EXACT.iter().any(|e| canon == Path::new(e)) {
        return forbid("system or top-level folders can't be organized");
    }
    if SUBTREE.iter().any(|s| canon.starts_with(s)) {
        return forbid("macOS system and application folders can't be organized");
    }
    if canon == home {
        return forbid(
            "choose specific folders inside your home folder, not the whole home folder",
        );
    }
    if canon.starts_with(home.join("Library")) {
        return forbid("the Library folder holds app data and can't be organized");
    }
    if canon.starts_with(&app_data) || app_data.starts_with(&canon) {
        return forbid("this folder contains FolderFlow's own data");
    }
    Ok(canon)
}

/// NFC-normalises a path. FolderFlow stores and compares paths in NFC.
pub fn nfc(p: &Path) -> PathBuf {
    PathBuf::from(nfc_str(&p.to_string_lossy()))
}

/// NFC-normalises a string slice (APFS stores decomposed NFD names).
pub fn nfc_str(s: &str) -> String {
    s.nfc().collect()
}

pub fn lexical_check(path: &Path) -> Result<(), PathError> {
    if !path.is_absolute() {
        return Err(PathError::NotAbsolute);
    }
    if path.as_os_str().len() > MAX_PATH_BYTES {
        return Err(PathError::TooLong);
    }
    for c in path.components() {
        match c {
            Component::RootDir => {}
            Component::Normal(s) => validate_component(utf8(s)?)?,
            Component::CurDir | Component::ParentDir => {
                return Err(PathError::InvalidComponent(
                    c.as_os_str().to_string_lossy().into_owned(),
                ))
            }
            Component::Prefix(_) => return Err(PathError::NotAbsolute),
        }
    }
    Ok(())
}

fn utf8(s: &OsStr) -> Result<&str, PathError> {
    s.to_str()
        .ok_or_else(|| PathError::InvalidComponent("name is not valid UTF-8".into()))
}

/// Canonicalises the deepest existing ancestor of the parent, then re-appends
/// the not-yet-existing components and the (unresolved) leaf.
fn resolve(path: &Path) -> Result<(PathBuf, bool), PathError> {
    let Some(leaf) = path.file_name() else {
        return Ok((PathBuf::from("/"), false)); // the root directory itself
    };
    let parent = path.parent().unwrap_or(Path::new("/"));

    let mut existing = parent;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                missing.push(existing.file_name().ok_or(PathError::NotFound)?);
                existing = existing.parent().ok_or(PathError::NotFound)?;
            }
            Err(e) => return Err(e.into()),
        }
    }

    let mut resolved = existing.canonicalize()?;
    if !missing.is_empty() && !resolved.is_dir() {
        return Err(PathError::NotADirectory);
    }
    for part in missing.iter().rev() {
        resolved.push(part);
    }
    resolved.push(leaf);

    let leaf_is_symlink = std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    Ok((resolved, leaf_is_symlink))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{mkdir, tempdir, write_file};
    use proptest::prelude::*;
    use unicode_normalization::UnicodeNormalization;

    fn authority(root: &Path) -> PathAuthority {
        PathAuthority::new(vec![ApprovedRoot {
            path: root.to_path_buf(),
            kind: RootKind::Monitored,
        }])
    }

    #[test]
    fn r_path_01_relative_paths_are_rejected() {
        let (_d, root) = tempdir();
        assert_eq!(
            authority(&root).authorize(Path::new("Downloads/a.pdf")),
            Err(PathError::NotAbsolute)
        );
    }

    #[test]
    fn r_path_02_parent_traversal_is_rejected_even_if_it_resolves_inside() {
        let (_d, root) = tempdir();
        mkdir(&root.join("a"));
        let auth = authority(&root);
        assert!(matches!(
            auth.authorize(&root.join("a/../a/x.pdf")),
            Err(PathError::InvalidComponent(_))
        ));
        assert!(matches!(
            auth.authorize(&root.join("../escape.pdf")),
            Err(PathError::InvalidComponent(_))
        ));
    }

    #[test]
    fn r_path_03_inside_accepted_outside_rejected_nonexistent_leaf_ok() {
        let (_d, root) = tempdir();
        let (_o, other) = tempdir();
        let auth = authority(&root);
        let ok = auth
            .authorize(&root.join("Invoices/2026/new.pdf"))
            .expect("inside");
        assert_eq!(ok.path, root.join("Invoices/2026/new.pdf"));
        assert_eq!(ok.root, root);
        assert!(!ok.leaf_is_symlink);
        assert_eq!(
            auth.authorize(&other.join("x.pdf")),
            Err(PathError::OutsideApprovedRoots)
        );
        assert_eq!(
            auth.authorize(&root),
            Ok(AuthorizedPath {
                path: root.clone(),
                root: root.clone(),
                leaf_is_symlink: false
            })
        );
    }

    #[test]
    fn r_path_04_symlinked_directory_escaping_the_root_is_rejected() {
        let (_d, root) = tempdir();
        let (_o, outside) = tempdir();
        crate::test_support::symlink(&outside, root.join("link"));
        let auth = authority(&root);
        assert_eq!(
            auth.authorize(&root.join("link/secret.txt")),
            Err(PathError::OutsideApprovedRoots)
        );
        assert_eq!(
            auth.authorize(&root.join("link/new/dir/x")),
            Err(PathError::OutsideApprovedRoots)
        );
    }

    #[test]
    fn r_path_05_symlink_leaf_is_not_followed() {
        let (_d, root) = tempdir();
        let (_o, outside) = tempdir();
        write_file(&outside.join("target.txt"), b"x");
        crate::test_support::symlink(outside.join("target.txt"), root.join("link.txt"));
        let got = authority(&root)
            .authorize(&root.join("link.txt"))
            .expect("leaf link inside root is a valid item");
        assert_eq!(got.path, root.join("link.txt"));
        assert!(got.leaf_is_symlink);
    }

    #[test]
    fn r_path_06_unicode_nfc_and_nfd_spellings_are_equivalent() {
        let (_d, base) = tempdir();
        let nfc: String = "Café Docs".nfc().collect();
        let nfd: String = "Café Docs".nfd().collect();
        assert_ne!(nfc, nfd);
        mkdir(&base.join(&nfc));
        let auth = authority(&base.join(&nfd));
        assert!(auth.authorize(&base.join(&nfc).join("a.pdf")).is_ok());
        assert!(auth.authorize(&base.join(&nfd).join("a.pdf")).is_ok());
    }

    #[test]
    fn r_path_07_sibling_with_common_string_prefix_is_outside() {
        let (_d, base) = tempdir();
        mkdir(&base.join("data"));
        mkdir(&base.join("data2"));
        let auth = authority(&base.join("data"));
        assert_eq!(
            auth.authorize(&base.join("data2/f.txt")),
            Err(PathError::OutsideApprovedRoots)
        );
    }

    #[test]
    fn r_path_08_root_policy_rejects_dangerous_folders() {
        let (_d, base) = tempdir();
        let home = base.join("home");
        let app_data = home.join("Library/Application Support/com.folderflow.desktop");
        mkdir(&app_data);
        mkdir(&home.join("Downloads"));
        write_file(&home.join("file.txt"), b"x");
        let policy = RootPolicy {
            home: home.clone(),
            app_data_dir: app_data.clone(),
        };

        for p in [
            "/",
            "/System",
            "/System/Library",
            "/usr/local",
            "/Library",
            "/private",
            "/Users",
            "/Volumes",
            "/Applications",
        ] {
            assert!(
                matches!(
                    validate_new_root(Path::new(p), &policy),
                    Err(PathError::ForbiddenRoot(_))
                ),
                "{p}"
            );
        }
        assert!(matches!(
            validate_new_root(&home, &policy),
            Err(PathError::ForbiddenRoot(_))
        ));
        assert!(matches!(
            validate_new_root(&home.join("Library"), &policy),
            Err(PathError::ForbiddenRoot(_))
        ));
        assert!(matches!(
            validate_new_root(&app_data, &policy),
            Err(PathError::ForbiddenRoot(_))
        ));
        assert_eq!(
            validate_new_root(&home.join("file.txt"), &policy),
            Err(PathError::NotADirectory)
        );
        assert_eq!(
            validate_new_root(&home.join("missing"), &policy),
            Err(PathError::NotFound)
        );
        assert_eq!(
            validate_new_root(Path::new("rel/dir"), &policy),
            Err(PathError::NotAbsolute)
        );
        assert_eq!(
            validate_new_root(&home.join("Downloads"), &policy),
            Ok(home.join("Downloads"))
        );
    }

    #[test]
    fn r_path_08b_component_validation() {
        for bad in ["", ".", "..", "a/b", "a\0b", &"x".repeat(256)] {
            assert!(validate_component(bad).is_err(), "{bad:?}");
        }
        for good in [
            "invoice.pdf",
            "Café",
            "2026",
            ".hidden",
            "a b (1).txt",
            &"x".repeat(255),
        ] {
            assert!(validate_component(good).is_ok(), "{good:?}");
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]
        /// R-PATH proptest: no generated path ever authorises outside the root.
        #[test]
        fn r_path_prop_never_escapes_root(parts in proptest::collection::vec(
            prop_oneof![
                Just("..".to_string()), Just(".".to_string()), Just("".to_string()),
                "[a-zA-Z0-9 ._-]{1,12}", "\\PC{1,8}", Just("a/../..".to_string()),
            ], 1..6)) {
            let (_d, root) = tempdir();
            let auth = authority(&root);
            let mut p = root.clone();
            for part in &parts { p.push(part); }
            if let Ok(ok) = auth.authorize(&p) {
                prop_assert!(ok.path.starts_with(&root), "{:?} escaped to {:?}", p, ok.path);
                prop_assert!(!ok.path.components().any(|c| matches!(c, std::path::Component::ParentDir)));
            }
        }
    }
}
