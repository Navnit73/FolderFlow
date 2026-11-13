//! File classification (FR-030 – FR-035).
//!
//! Combines the file extension with the content signature (magic bytes via
//! `infer`). Files are never executed. When extension and content disagree in
//! a way that isn't a known container relationship (e.g. `.docx` is a zip),
//! the file is classified `other` with low confidence so rules leave it alone
//! unless they explicitly target it (FR-032).

use serde::Serialize;
use specta::Type;

use crate::categories::{builtin_category_for_extension, Category, OTHER};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Extension,
    Signature,
    Package,
    /// Extension and content disagree (e.g. a program named `.pdf`).
    Mismatch,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub category: String,
    pub mime: Option<String>,
    pub confidence: Confidence,
    pub reason: Reason,
}

/// Bytes read from the start of a file for signature detection.
pub const HEADER_BYTES: usize = 8192;

/// Extensions whose files are zip containers.
const ZIP_CONTAINERS: &[&str] = &[
    "docx", "xlsx", "pptx", "odt", "ods", "odp", "pages", "numbers", "key", "epub", "sketch",
    "fig", "xd", "jar", "ipa", "apk",
];

/// (extension category, signature category) pairs that are legitimately compatible.
const COMPATIBLE: &[(&str, &str)] = &[
    ("design", "images"),       // PSD, AI previews
    ("design", "documents"),    // AI/EPS are PDF/PostScript-based
    ("installers", "archives"), // .pkg (xar), compressed .dmg
    ("ebooks", "archives"),
    ("documents", "spreadsheets"),
    ("documents", "presentations"),
    ("spreadsheets", "documents"), // legacy OLE Office files share one signature
    ("presentations", "documents"),
    ("spreadsheets", "presentations"),
    ("presentations", "spreadsheets"),
];

/// Text signatures (html, xml, shell) fit these extension categories.
const TEXT_COMPATIBLE: &[&str] = &[
    "development",
    "documents",
    "images", /* svg */
    "spreadsheets",
];

#[derive(Debug, PartialEq, Eq)]
enum Signature {
    Category(&'static str),
    Zip,
    Text,
    Executable,
}

fn signature_of(kind: &infer::Type) -> Signature {
    match kind.mime_type() {
        "application/pdf" | "application/rtf" | "application/postscript" => {
            return Signature::Category("documents")
        }
        "application/zip" => return Signature::Zip,
        "application/epub+zip" | "application/x-mobipocket-ebook" => {
            return Signature::Category("ebooks")
        }
        "application/msword" | "application/x-ole-storage" => {
            return Signature::Category("documents")
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            return Signature::Category("spreadsheets")
        }
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            return Signature::Category("presentations")
        }
        "application/vnd.sqlite3" => return Signature::Category(OTHER),
        _ => {}
    }
    match kind.matcher_type() {
        infer::MatcherType::App => Signature::Executable,
        infer::MatcherType::Archive => Signature::Category("archives"),
        infer::MatcherType::Audio => Signature::Category("audio"),
        infer::MatcherType::Video => Signature::Category("videos"),
        infer::MatcherType::Image => Signature::Category("images"),
        infer::MatcherType::Font => Signature::Category("fonts"),
        infer::MatcherType::Book => Signature::Category("ebooks"),
        infer::MatcherType::Doc => Signature::Category("documents"),
        infer::MatcherType::Text => Signature::Text,
        infer::MatcherType::Custom => Signature::Category(OTHER),
    }
}

fn compatible(ext: &str, ext_category: &str, sig: &Signature) -> bool {
    match sig {
        Signature::Executable => false,
        Signature::Zip => ext_category == "archives" || ZIP_CONTAINERS.contains(&ext),
        Signature::Text => TEXT_COMPATIBLE.contains(&ext_category),
        Signature::Category(c) => *c == ext_category || COMPATIBLE.contains(&(ext_category, *c)),
    }
}

pub struct Classifier {
    /// Enabled categories in match order: priority asc, custom before built-in, then id.
    ordered: Vec<Category>,
}

impl Classifier {
    pub fn new(mut categories: Vec<Category>) -> Self {
        categories.retain(|c| c.enabled);
        categories
            .sort_by(|a, b| (a.priority, a.builtin, &a.id).cmp(&(b.priority, b.builtin, &b.id)));
        Self {
            ordered: categories,
        }
    }

    fn category_for_extension(&self, ext: &str) -> Option<&Category> {
        self.ordered
            .iter()
            .find(|c| c.extensions.iter().any(|e| e == ext))
    }

    fn is_enabled(&self, id: &str) -> bool {
        self.ordered.iter().any(|c| c.id == id)
    }

    /// `header` is `None` when content wasn't read (e.g. packages); `Some(&[])` for empty files.
    pub fn classify(
        &self,
        file_name: &str,
        header: Option<&[u8]>,
        is_package: bool,
    ) -> Classification {
        let ext = extension_of(file_name);
        let sig_type = header.and_then(infer::get);
        let mime = sig_type.as_ref().map(|t| t.mime_type().to_owned());
        let sig = sig_type.as_ref().map(signature_of);
        let other = |confidence, reason| Classification {
            category: OTHER.to_owned(),
            mime: mime.clone(),
            confidence,
            reason,
        };

        if is_package {
            return match ext.as_deref().and_then(|e| self.category_for_extension(e)) {
                Some(c) => Classification {
                    category: c.id.clone(),
                    mime: None,
                    confidence: Confidence::High,
                    reason: Reason::Package,
                },
                None => other(Confidence::Low, Reason::Package),
            };
        }

        let Some(ext) = ext else {
            // No extension: trust a concrete content signature, nothing else.
            return match sig {
                Some(Signature::Category(c)) if c != OTHER && self.is_enabled(c) => {
                    Classification {
                        category: c.to_owned(),
                        mime: mime.clone(),
                        confidence: Confidence::High,
                        reason: Reason::Signature,
                    }
                }
                Some(Signature::Executable) => other(Confidence::Low, Reason::Mismatch),
                _ => other(Confidence::Low, Reason::Unknown),
            };
        };

        // Content must be compatible with what the extension claims. Checked
        // against the built-in meaning of the extension so custom categories
        // can't bypass it.
        if let Some(sig) = &sig {
            let claimed = builtin_category_for_extension(&ext);
            let ok = match claimed {
                Some(claimed) => compatible(&ext, claimed, sig),
                None => !matches!(sig, Signature::Executable),
            };
            if !ok {
                return other(Confidence::Low, Reason::Mismatch);
            }
        }

        match self.category_for_extension(&ext) {
            Some(c) => Classification {
                category: c.id.clone(),
                mime,
                confidence: Confidence::High,
                reason: Reason::Extension,
            },
            None => match sig {
                Some(Signature::Category(c)) if c != OTHER && self.is_enabled(c) => {
                    Classification {
                        category: c.to_owned(),
                        mime: mime.clone(),
                        confidence: Confidence::High,
                        reason: Reason::Signature,
                    }
                }
                _ => other(Confidence::Low, Reason::Unknown),
            },
        }
    }
}

/// Lower-cased last extension, if any (`archive.tar.gz` → `gz`; `.bashrc` → none).
pub fn extension_of(file_name: &str) -> Option<String> {
    let (stem, ext) = file_name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() {
        return None;
    }
    Some(ext.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::builtin_categories;
    use crate::test_support::magic;

    fn builtin() -> Classifier {
        Classifier::new(builtin_categories())
    }

    fn cat(c: &Classifier, name: &str, header: Option<&[u8]>) -> (String, Confidence, Reason) {
        let r = c.classify(name, header, false);
        (r.category, r.confidence, r.reason)
    }

    #[test]
    fn r_cls_01_every_builtin_category_by_extension() {
        let c = builtin();
        for (name, expected) in [
            ("report.PDF", "documents"),
            ("notes.txt", "documents"),
            ("photo.HEIC", "images"),
            ("clip.mov", "videos"),
            ("song.flac", "audio"),
            ("backup.7z", "archives"),
            ("budget.csv", "spreadsheets"),
            ("deck.key", "presentations"),
            ("Setup.dmg", "installers"),
            ("mock.sketch", "design"),
            ("main.rs", "development"),
            ("Inter.woff2", "fonts"),
            ("novel.epub", "ebooks"),
        ] {
            assert_eq!(
                cat(&c, name, None),
                (expected.into(), Confidence::High, Reason::Extension),
                "{name}"
            );
        }
    }

    #[test]
    fn r_cls_02_matching_signature_is_high_confidence_with_mime() {
        let r = builtin().classify("invoice.pdf", Some(magic::PDF), false);
        assert_eq!(
            (r.category.as_str(), r.confidence, r.reason),
            ("documents", Confidence::High, Reason::Extension)
        );
        assert_eq!(r.mime.as_deref(), Some("application/pdf"));
        assert_eq!(
            builtin()
                .classify("a.jpg", Some(magic::JPEG), false)
                .category,
            "images"
        );
        assert_eq!(
            builtin()
                .classify("a.mp4", Some(magic::MP4), false)
                .category,
            "videos"
        );
        assert_eq!(
            builtin()
                .classify("a.tar.gz", Some(magic::GZIP), false)
                .category,
            "archives"
        );
    }

    #[test]
    fn r_cls_03_program_disguised_as_document_is_other_low() {
        assert_eq!(
            cat(&builtin(), "invoice.pdf", Some(magic::MACHO_64)),
            ("other".into(), Confidence::Low, Reason::Mismatch)
        );
        assert_eq!(
            cat(&builtin(), "tool", Some(magic::MACHO_64)),
            ("other".into(), Confidence::Low, Reason::Mismatch)
        );
    }

    #[test]
    fn r_cls_04_extension_signature_conflict_is_other_low() {
        assert_eq!(
            cat(&builtin(), "photo.pdf", Some(magic::PNG)),
            ("other".into(), Confidence::Low, Reason::Mismatch)
        );
        assert_eq!(
            cat(&builtin(), "notes.txt", Some(magic::ZIP)),
            ("other".into(), Confidence::Low, Reason::Mismatch)
        );
    }

    #[test]
    fn r_cls_05_no_extension_uses_signature() {
        assert_eq!(
            cat(&builtin(), "IMG_0001", Some(magic::PNG)),
            ("images".into(), Confidence::High, Reason::Signature)
        );
    }

    #[test]
    fn r_cls_06_unknown_is_other_low() {
        assert_eq!(
            cat(&builtin(), "data.qqq", None),
            ("other".into(), Confidence::Low, Reason::Unknown)
        );
        assert_eq!(
            cat(&builtin(), "data.qqq", Some(b"hello world")),
            ("other".into(), Confidence::Low, Reason::Unknown)
        );
        assert_eq!(
            cat(&builtin(), "README", Some(b"plain text")),
            ("other".into(), Confidence::Low, Reason::Unknown)
        );
    }

    #[test]
    fn r_cls_07_zip_based_formats_keep_their_category() {
        for (name, expected) in [
            ("report.docx", "documents"),
            ("sheet.xlsx", "spreadsheets"),
            ("talk.key", "presentations"),
            ("book.epub", "ebooks"),
            ("design.sketch", "design"),
            ("files.zip", "archives"),
            ("numbers.numbers", "spreadsheets"),
        ] {
            assert_eq!(
                cat(&builtin(), name, Some(magic::ZIP)).0,
                expected,
                "{name}"
            );
        }
        // Illustrator files are PDF-compatible; PSD is image-like.
        assert_eq!(cat(&builtin(), "logo.ai", Some(magic::PDF)).0, "design");
    }

    #[test]
    fn r_cls_08_empty_files_packages_and_extension_helper() {
        assert_eq!(
            cat(&builtin(), "empty.txt", Some(&[])),
            ("documents".into(), Confidence::High, Reason::Extension)
        );
        let r = builtin().classify("Slack.app", None, true);
        assert_eq!(
            (r.category.as_str(), r.reason),
            ("installers", Reason::Package)
        );
        assert_eq!(extension_of("a.tar.gz").as_deref(), Some("gz"));
        assert_eq!(extension_of("Photo.JPG").as_deref(), Some("jpg"));
        assert_eq!(extension_of(".bashrc"), None);
        assert_eq!(extension_of("noext"), None);
        assert_eq!(extension_of("trailing."), None);
    }

    #[test]
    fn r_cat_01_custom_categories_take_precedence_by_priority() {
        let mut cats = builtin_categories();
        let custom = |id: &str, prio: i32, enabled: bool| Category {
            id: id.into(),
            name: id.into(),
            description: None,
            icon: "receipt".into(),
            color_token: "cat-documents".into(),
            extensions: vec!["pdf".into()],
            destination_path: None,
            priority: prio,
            builtin: false,
            enabled,
        };
        cats.push(custom("receipts", 60, true));
        cats.push(custom("invoices", 10, true));
        cats.push(custom("disabled", 1, false));
        let c = Classifier::new(cats);
        assert_eq!(
            c.classify("a.pdf", Some(magic::PDF), false).category,
            "invoices"
        );
        // Safety check still applies to custom categories.
        assert_eq!(
            c.classify("a.pdf", Some(magic::MACHO_64), false).category,
            "other"
        );
        // Disabled built-in falls through to Other.
        let mut cats = builtin_categories();
        cats.iter_mut()
            .find(|c| c.id == "images")
            .expect("images")
            .enabled = false;
        assert_eq!(
            Classifier::new(cats)
                .classify("a.png", None, false)
                .category,
            "other"
        );
    }
}
