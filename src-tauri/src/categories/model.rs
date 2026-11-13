use serde::{Deserialize, Serialize};
use specta::Type;

/// A file category (built-in or custom). `id` is stable; built-ins use their slug.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Lucide icon name, e.g. `file-text`.
    pub icon: String,
    /// Theme token name, e.g. `cat-images` (never a raw color — ADR-003).
    pub color_token: String,
    /// Lower-case extensions without dots.
    pub extensions: Vec<String>,
    pub destination_path: Option<String>,
    /// Lower number = matched first. Custom categories default to 50, built-ins 100.
    pub priority: i32,
    pub builtin: bool,
    pub enabled: bool,
}

/// Input for creating or editing a custom category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInput {
    pub name: String,
    pub description: Option<String>,
    pub icon: String,
    pub color_token: String,
    pub extensions: Vec<String>,
    pub destination_path: Option<String>,
    pub priority: i32,
    pub enabled: bool,
}

pub const OTHER: &str = "other";

/// Category color tokens defined in src/styles/theme.css.
pub const COLOR_TOKENS: &[&str] = &[
    "cat-documents",
    "cat-images",
    "cat-videos",
    "cat-audio",
    "cat-archives",
    "cat-spreadsheets",
    "cat-presentations",
    "cat-installers",
    "cat-design",
    "cat-development",
    "cat-fonts",
    "cat-ebooks",
    "cat-other",
];

struct Builtin {
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    extensions: &'static [&'static str],
}

const BUILTINS: &[Builtin] = &[
    Builtin {
        id: "documents",
        name: "Documents",
        icon: "file-text",
        extensions: &["pdf", "doc", "docx", "txt", "rtf", "odt", "pages"],
    },
    Builtin {
        id: "images",
        name: "Images",
        icon: "image",
        extensions: &[
            "png", "jpg", "jpeg", "webp", "heic", "heif", "gif", "svg", "tiff", "tif", "bmp", "ico",
        ],
    },
    Builtin {
        id: "videos",
        name: "Videos",
        icon: "film",
        extensions: &["mp4", "mov", "mkv", "avi", "webm", "m4v"],
    },
    Builtin {
        id: "audio",
        name: "Audio",
        icon: "music",
        extensions: &["mp3", "wav", "aac", "flac", "m4a", "ogg", "aiff", "aif"],
    },
    Builtin {
        id: "archives",
        name: "Archives",
        icon: "archive",
        extensions: &["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz"],
    },
    Builtin {
        id: "spreadsheets",
        name: "Spreadsheets",
        icon: "sheet",
        extensions: &["xls", "xlsx", "csv", "ods", "numbers"],
    },
    Builtin {
        id: "presentations",
        name: "Presentations",
        icon: "presentation",
        extensions: &["ppt", "pptx", "key", "odp"],
    },
    Builtin {
        id: "installers",
        name: "Applications & Installers",
        icon: "package",
        extensions: &["dmg", "pkg", "app", "mpkg"],
    },
    Builtin {
        id: "design",
        name: "Design",
        icon: "pen-tool",
        extensions: &["psd", "ai", "eps", "sketch", "fig", "xd", "aep"],
    },
    Builtin {
        id: "development",
        name: "Development",
        icon: "code",
        extensions: &[
            "js", "ts", "jsx", "tsx", "html", "htm", "css", "scss", "json", "xml", "py", "php",
            "go", "rs", "java", "c", "h", "cpp", "hpp", "sql", "md", "sh", "yml", "yaml", "toml",
        ],
    },
    Builtin {
        id: "fonts",
        name: "Fonts",
        icon: "type",
        extensions: &["ttf", "otf", "woff", "woff2"],
    },
    Builtin {
        id: "ebooks",
        name: "E-books",
        icon: "book-open",
        extensions: &["epub", "mobi", "azw", "azw3"],
    },
    Builtin {
        id: OTHER,
        name: "Other",
        icon: "file",
        extensions: &[],
    },
];

/// The built-in categories (FR-030) with default settings.
pub fn builtin_categories() -> Vec<Category> {
    BUILTINS
        .iter()
        .map(|b| Category {
            id: b.id.to_owned(),
            name: b.name.to_owned(),
            description: None,
            icon: b.icon.to_owned(),
            color_token: format!("cat-{}", b.id),
            extensions: b.extensions.iter().map(|e| (*e).to_owned()).collect(),
            destination_path: None,
            priority: 100,
            builtin: true,
            enabled: true,
        })
        .collect()
}

/// Built-in category id for an extension, ignoring user customisation.
pub fn builtin_category_for_extension(ext: &str) -> Option<&'static str> {
    BUILTINS
        .iter()
        .find(|b| b.extensions.contains(&ext))
        .map(|b| b.id)
}
