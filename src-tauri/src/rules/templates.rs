//! Dynamic path and filename template engine (P3-T05, FR-070–076).
//!
//! Renders `{year}`, `{month}`, `{month_name}`, `{day}`, `{date}`, `{extension}`,
//! `{category}`, `{original_name}`, `{source_folder}` using the rule's `DateSource`
//! and injectable `Clock`. Variable values are sanitized before substitution so
//! file metadata can never inject path separators, traversal (`..`), `:`, or NUL.

use std::path::Path;

use chrono::Datelike;
use serde::{Deserialize, Serialize};
use specta::Type;
use unicode_normalization::UnicodeNormalization;

use crate::engine::paths::{
    AuthorizedPath, PathAuthority, PathError, MAX_COMPONENT_BYTES, MAX_PATH_BYTES,
};
use crate::rules::models::{DateSource, FileFacts};
use crate::utils::clock::Clock;

pub const KNOWN_VARIABLES: &[&str] = &[
    "year",
    "month",
    "month_name",
    "day",
    "date",
    "extension",
    "category",
    "original_name",
    "source_folder",
];

/// Built-in naming template presets (FR-076).
pub const NAMING_TEMPLATE_PRESETS: &[(&str, &str)] = &[
    ("date_name", "{date}_{original_name}"),
    ("category_name", "{category}_{original_name}"),
    ("year_month_name", "{year}-{month}_{original_name}"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, thiserror::Error)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum TemplateError {
    #[error("template cannot be empty")]
    EmptyTemplate,
    #[error("unclosed '{{' in template")]
    UnclosedBrace,
    #[error("unexpected '}}' in template")]
    UnexpectedClosingBrace,
    #[error("unknown template variable '{{{0}}}'")]
    UnknownVariable(String),
    #[error("path traversal ('..') is not allowed in templates")]
    TraversalNotAllowed,
    #[error("invalid path or filename component: {0}")]
    InvalidComponent(String),
    #[error("name component is too long ({0} bytes, max 255)")]
    ComponentTooLong(usize),
    #[error("generated path is too long ({0} bytes, max 1024)")]
    PathTooLong(usize),
    #[error("destination is outside approved folders")]
    OutsideApprovedRoots,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateToken {
    Literal(String),
    Variable(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SampleFile {
    pub name: String,
    pub category: String,
    pub source_folder: String,
    #[serde(default)]
    #[specta(type = Option<f64>)]
    pub modified_ms: Option<i64>,
    #[serde(default)]
    #[specta(type = Option<f64>)]
    pub created_ms: Option<i64>,
    #[serde(default)]
    #[specta(type = Option<f64>)]
    pub added_ms: Option<i64>,
}

impl SampleFile {
    pub fn to_facts(&self, fallback_ms: i64) -> FileFacts {
        let (stem, extension) = FileFacts::split_name(&self.name);
        let modified_ms = self.modified_ms.unwrap_or(fallback_ms);
        FileFacts {
            path: format!("/Users/sample/{}/{}", self.source_folder, self.name),
            name: self.name.clone(),
            stem,
            extension,
            mime: None,
            category: self.category.clone(),
            size: 1024,
            created_ms: self.created_ms.or(Some(modified_ms)),
            modified_ms,
            added_ms: self.added_ms.or(Some(modified_ms)),
            is_hidden: self.name.starts_with('.'),
            is_duplicate: false,
            content_hash: None,
            source_folder_id: "sample-folder".into(),
            source_folder_path: format!("/Users/sample/{}", self.source_folder),
            source_folder_name: self.source_folder.clone(),
        }
    }
}

pub fn parse_template_tokens(template: &str) -> Result<Vec<TemplateToken>, TemplateError> {
    let trimmed = template.trim();
    if trimmed.is_empty() {
        return Err(TemplateError::EmptyTemplate);
    }

    let mut tokens = Vec::new();
    let mut literal = String::new();
    let mut chars = trimmed.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            if !literal.is_empty() {
                tokens.push(TemplateToken::Literal(std::mem::take(&mut literal)));
            }
            let mut var_name = String::new();
            let mut closed = false;
            for vch in chars.by_ref() {
                if vch == '}' {
                    closed = true;
                    break;
                }
                if vch == '{' {
                    return Err(TemplateError::UnclosedBrace);
                }
                var_name.push(vch);
            }
            if !closed {
                return Err(TemplateError::UnclosedBrace);
            }
            let var_trimmed = var_name.trim();
            if !KNOWN_VARIABLES.contains(&var_trimmed) {
                return Err(TemplateError::UnknownVariable(var_trimmed.to_owned()));
            }
            tokens.push(TemplateToken::Variable(var_trimmed.to_owned()));
        } else if ch == '}' {
            return Err(TemplateError::UnexpectedClosingBrace);
        } else {
            literal.push(ch);
        }
    }

    if !literal.is_empty() {
        tokens.push(TemplateToken::Literal(literal));
    }

    Ok(tokens)
}

/// Sanitizes a variable value so it can never inject path separators, traversal,
/// colons, NULs, control characters, or empty components (FR-072).
pub fn sanitize_variable_value(raw: &str, fallback: &str) -> String {
    let nfc: String = raw.nfc().collect();
    let mut cleaned = String::with_capacity(nfc.len());
    for ch in nfc.chars() {
        if ch == '/' || ch == '\\' || ch == ':' || ch == '\0' || ch.is_control() {
            cleaned.push('_');
        } else {
            cleaned.push(ch);
        }
    }
    // Collapse any `..` inside variable values so traversal can never be formed
    while cleaned.contains("..") {
        cleaned = cleaned.replace("..", "_");
    }
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn resolve_date_ms(facts: &FileFacts, date_source: DateSource, clock: &dyn Clock) -> i64 {
    match date_source {
        DateSource::Modified => facts.modified_ms,
        DateSource::Created => facts.created_ms.unwrap_or(facts.modified_ms),
        DateSource::Added => facts.added_ms.unwrap_or(facts.modified_ms),
        DateSource::Now => clock.now_ms(),
    }
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Unknown",
    }
}

fn expand_variable(
    name: &str,
    facts: &FileFacts,
    date_source: DateSource,
    clock: &dyn Clock,
    allow_empty_extension: bool,
) -> String {
    let ts_ms = resolve_date_ms(facts, date_source, clock);
    let local_date = clock.local_date(ts_ms);

    match name {
        "year" => format!("{:04}", local_date.year()),
        "month" => format!("{:02}", local_date.month()),
        "month_name" => month_name(local_date.month()).to_owned(),
        "day" => format!("{:02}", local_date.day()),
        "date" => format!(
            "{:04}-{:02}-{:02}",
            local_date.year(),
            local_date.month(),
            local_date.day()
        ),
        "extension" => {
            let ext_raw = facts.extension.trim().trim_start_matches('.');
            if ext_raw.is_empty() && allow_empty_extension {
                String::new()
            } else {
                sanitize_variable_value(&ext_raw.to_lowercase(), "unknown")
            }
        }
        "category" => sanitize_variable_value(&facts.category, "Other"),
        "original_name" => {
            let stem = if !facts.stem.is_empty() {
                facts.stem.as_str()
            } else {
                facts.name.as_str()
            };
            sanitize_variable_value(stem, "file")
        }
        "source_folder" => sanitize_variable_value(&facts.source_folder_name, "Folder"),
        _ => String::new(),
    }
}

fn validate_rendered_component(seg: &str) -> Result<String, TemplateError> {
    if seg == "." || seg == ".." {
        return Err(TemplateError::TraversalNotAllowed);
    }
    let trimmed = seg.trim();
    if trimmed.is_empty() {
        return Err(TemplateError::InvalidComponent(
            "empty path component".into(),
        ));
    }
    if trimmed == "." || trimmed == ".." {
        return Err(TemplateError::TraversalNotAllowed);
    }
    if trimmed.contains(':') || trimmed.contains('\0') || trimmed.contains('/') {
        return Err(TemplateError::InvalidComponent(trimmed.to_owned()));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(TemplateError::InvalidComponent(
            "control characters are not allowed".into(),
        ));
    }
    if trimmed.len() > MAX_COMPONENT_BYTES {
        return Err(TemplateError::ComponentTooLong(trimmed.len()));
    }
    Ok(trimmed.to_owned())
}

pub fn render_subpath_template(
    template: &str,
    facts: &FileFacts,
    date_source: DateSource,
    clock: &dyn Clock,
) -> Result<String, TemplateError> {
    let trimmed = template.trim();
    if trimmed.is_empty() {
        return Err(TemplateError::EmptyTemplate);
    }
    if trimmed.starts_with('/') || trimmed.ends_with('/') || trimmed.contains("//") {
        return Err(TemplateError::InvalidComponent(
            "empty path component".into(),
        ));
    }

    // Parse full template first to validate all braces & variable names
    let _ = parse_template_tokens(trimmed)?;

    let mut rendered_segments = Vec::new();
    for raw_seg in trimmed.split('/') {
        if raw_seg == "." || raw_seg == ".." {
            return Err(TemplateError::TraversalNotAllowed);
        }
        let tokens = parse_template_tokens(raw_seg)?;
        let mut out = String::new();
        for tok in &tokens {
            match tok {
                TemplateToken::Literal(lit) => {
                    if lit.contains(':') || lit.contains('\0') || lit.chars().any(char::is_control)
                    {
                        return Err(TemplateError::InvalidComponent(lit.clone()));
                    }
                    out.push_str(&lit.nfc().collect::<String>());
                }
                TemplateToken::Variable(var) => {
                    let allow_empty_ext = var == "extension" && tokens.len() > 1;
                    out.push_str(&expand_variable(
                        var,
                        facts,
                        date_source,
                        clock,
                        allow_empty_ext,
                    ));
                }
            }
        }
        let cleaned = out.trim().trim_end_matches('.');
        let seg = validate_rendered_component(cleaned)?;
        rendered_segments.push(seg);
    }

    let joined = rendered_segments.join("/");
    if joined.len() > MAX_PATH_BYTES {
        return Err(TemplateError::PathTooLong(joined.len()));
    }
    Ok(joined)
}

pub fn render_filename_template(
    template: &str,
    facts: &FileFacts,
    date_source: DateSource,
    clock: &dyn Clock,
) -> Result<String, TemplateError> {
    let trimmed = template.trim();
    if trimmed.is_empty() {
        return Err(TemplateError::EmptyTemplate);
    }
    if trimmed.contains('/') {
        return Err(TemplateError::InvalidComponent(
            "filename template cannot contain '/'".into(),
        ));
    }

    let tokens = parse_template_tokens(trimmed)?;
    let mentions_extension = tokens
        .iter()
        .any(|t| matches!(t, TemplateToken::Variable(v) if v == "extension"));

    let mut out = String::new();
    for tok in &tokens {
        match tok {
            TemplateToken::Literal(lit) => {
                if lit.contains(':') || lit.contains('\0') || lit.chars().any(char::is_control) {
                    return Err(TemplateError::InvalidComponent(lit.clone()));
                }
                out.push_str(&lit.nfc().collect::<String>());
            }
            TemplateToken::Variable(var) => {
                out.push_str(&expand_variable(var, facts, date_source, clock, true));
            }
        }
    }

    // Strip trailing dot if `{extension}` was empty
    let mut filename = out.trim().trim_end_matches('.').to_owned();

    // FR-076: If `{extension}` was not explicitly placed in the naming template
    // and the file has a valid extension, preserve the file's extension.
    let ext_clean =
        sanitize_variable_value(facts.extension.trim().trim_start_matches('.'), "").to_lowercase();
    if !mentions_extension && !ext_clean.is_empty() {
        let suffix = format!(".{ext_clean}");
        if !filename.to_lowercase().ends_with(&suffix) {
            filename.push_str(&suffix);
        }
    }

    validate_rendered_component(&filename)
}

pub fn resolve_destination_path(
    base_root: &Path,
    subpath_template: &str,
    filename: &str,
    facts: &FileFacts,
    date_source: DateSource,
    clock: &dyn Clock,
    authority: &PathAuthority,
) -> Result<AuthorizedPath, TemplateError> {
    let valid_filename = validate_rendered_component(filename)?;
    let mut full = base_root.to_path_buf();
    if !subpath_template.trim().is_empty() {
        let sub = render_subpath_template(subpath_template, facts, date_source, clock)?;
        full.push(sub);
    }
    full.push(valid_filename);

    if full.as_os_str().len() > MAX_PATH_BYTES {
        return Err(TemplateError::PathTooLong(full.as_os_str().len()));
    }

    authority.authorize(&full).map_err(|e| match e {
        PathError::OutsideApprovedRoots => TemplateError::OutsideApprovedRoots,
        PathError::TooLong => TemplateError::PathTooLong(full.as_os_str().len()),
        other => TemplateError::InvalidComponent(other.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::paths::{ApprovedRoot, RootKind};
    use crate::test_support::tempdir;
    use crate::utils::clock::FixedClock;
    use chrono::TimeZone;
    use proptest::prelude::*;

    fn utc_ms(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> i64 {
        chrono::Utc
            .with_ymd_and_hms(y, m, d, hh, mm, ss)
            .single()
            .expect("valid utc datetime")
            .timestamp_millis()
    }

    fn sample_facts() -> FileFacts {
        FileFacts {
            path: "/Users/alice/Downloads/Q4_Report.pdf".into(),
            name: "Q4_Report.pdf".into(),
            stem: "Q4_Report".into(),
            extension: "pdf".into(),
            mime: Some("application/pdf".into()),
            category: "Documents".into(),
            size: 120_000,
            created_ms: Some(utc_ms(2025, 1, 5, 10, 0, 0)),
            modified_ms: utc_ms(2026, 11, 14, 10, 0, 0),
            added_ms: Some(utc_ms(2026, 11, 15, 10, 0, 0)),
            is_hidden: false,
            is_duplicate: false,
            content_hash: None,
            source_folder_id: "f-dl".into(),
            source_folder_path: "/Users/alice/Downloads".into(),
            source_folder_name: "Downloads".into(),
        }
    }

    #[test]
    fn r_tpl_01_renders_all_variables_accurately() {
        let clock = FixedClock::utc(utc_ms(2026, 12, 25, 12, 0, 0));
        let facts = sample_facts();

        let rendered = render_subpath_template(
            "{source_folder}/{category}/{year}/{month}_{month_name}/{day}/{date}_{original_name}.{extension}",
            &facts,
            DateSource::Modified,
            &clock,
        )
        .expect("render subpath");

        assert_eq!(
            rendered,
            "Downloads/Documents/2026/11_November/14/2026-11-14_Q4_Report.pdf"
        );
    }

    #[test]
    fn r_tpl_02_respects_date_source_and_local_timezone() {
        // Now is 2026-12-25, Created is 2025-01-05, Modified is 2026-11-14, Added is 2026-11-15
        let clock = FixedClock::utc(utc_ms(2026, 12, 25, 12, 0, 0));
        let facts = sample_facts();

        assert_eq!(
            render_subpath_template("{date}", &facts, DateSource::Modified, &clock).expect("mod"),
            "2026-11-14"
        );
        assert_eq!(
            render_subpath_template("{date}", &facts, DateSource::Created, &clock).expect("cre"),
            "2025-01-05"
        );
        assert_eq!(
            render_subpath_template("{date}", &facts, DateSource::Added, &clock).expect("add"),
            "2026-11-15"
        );
        assert_eq!(
            render_subpath_template("{date}", &facts, DateSource::Now, &clock).expect("now"),
            "2026-12-25"
        );

        // Local timezone test: 2026-03-01T02:00:00Z in UTC-5 is 2026-02-28 (February!)
        let mut tz_facts = sample_facts();
        tz_facts.modified_ms = utc_ms(2026, 3, 1, 2, 0, 0);
        let ny_clock = FixedClock::with_offset_mins(utc_ms(2026, 3, 1, 2, 0, 0), -300);
        assert_eq!(
            render_subpath_template(
                "{year}/{month}/{month_name}/{day}",
                &tz_facts,
                DateSource::Modified,
                &ny_clock
            )
            .expect("tz render"),
            "2026/02/February/28"
        );
    }

    #[test]
    fn r_tpl_03_rejects_unknown_variables_and_malformed_braces() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let facts = sample_facts();

        assert_eq!(
            render_subpath_template("{unknown_var}", &facts, DateSource::Modified, &clock),
            Err(TemplateError::UnknownVariable("unknown_var".into()))
        );
        assert_eq!(
            render_subpath_template("Docs/{year", &facts, DateSource::Modified, &clock),
            Err(TemplateError::UnclosedBrace)
        );
        assert_eq!(
            render_subpath_template("Docs/year}", &facts, DateSource::Modified, &clock),
            Err(TemplateError::UnexpectedClosingBrace)
        );
        assert_eq!(
            render_subpath_template("", &facts, DateSource::Modified, &clock),
            Err(TemplateError::EmptyTemplate)
        );
    }

    #[test]
    fn r_tpl_04_sanitizes_variable_values_against_injection_and_invalid_chars() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let mut facts = sample_facts();
        // Malicious/weird stem, category, and source folder values
        facts.stem = "  ../../etc/passwd:evil\0name  ".into();
        facts.category = "../Secret:Cat".into();
        facts.source_folder_name = "..".into();

        let rendered = render_subpath_template(
            "{source_folder}/{category}/{original_name}",
            &facts,
            DateSource::Modified,
            &clock,
        )
        .expect("sanitized render");

        assert!(!rendered.contains(".."), "rendered: {rendered}");
        assert!(!rendered.contains(':'), "rendered: {rendered}");
        assert!(!rendered.contains('\0'), "rendered: {rendered}");
        // Must have exactly 3 components (none of the variables could inject extra '/')
        assert_eq!(rendered.split('/').count(), 3, "rendered: {rendered}");
    }

    #[test]
    fn r_tpl_05_rejects_traversal_and_invalid_literal_components() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let facts = sample_facts();

        assert_eq!(
            render_subpath_template("Docs/../Secret", &facts, DateSource::Modified, &clock),
            Err(TemplateError::TraversalNotAllowed)
        );
        assert_eq!(
            render_subpath_template("../Escape", &facts, DateSource::Modified, &clock),
            Err(TemplateError::TraversalNotAllowed)
        );
        assert!(matches!(
            render_subpath_template("Docs/Bad:Name", &facts, DateSource::Modified, &clock),
            Err(TemplateError::InvalidComponent(_))
        ));
        assert!(matches!(
            render_subpath_template("Docs//Empty", &facts, DateSource::Modified, &clock),
            Err(TemplateError::InvalidComponent(_))
        ));
    }

    #[test]
    fn r_tpl_06_enforces_component_255_and_path_1024_byte_limits() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let mut facts = sample_facts();
        facts.stem = "a".repeat(250);

        // Adding "{date}_" pushes the component over 255 bytes
        assert!(matches!(
            render_filename_template(
                "{date}_{original_name}",
                &facts,
                DateSource::Modified,
                &clock
            ),
            Err(TemplateError::ComponentTooLong(_))
        ));

        // Path > 1024 bytes
        let long_seg = "b".repeat(200);
        let deep_tpl = format!("{long_seg}/{long_seg}/{long_seg}/{long_seg}/{long_seg}/{long_seg}");
        assert!(matches!(
            render_subpath_template(&deep_tpl, &facts, DateSource::Modified, &clock),
            Err(TemplateError::PathTooLong(_))
        ));
    }

    #[test]
    fn r_tpl_07_resolves_destination_inside_approved_root_and_rejects_outside() {
        let (_d, root) = tempdir();
        let (_o, outside) = tempdir();
        let auth = PathAuthority::new(vec![ApprovedRoot {
            path: root.clone(),
            kind: RootKind::Destination,
        }]);
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let facts = sample_facts();

        let ok = resolve_destination_path(
            &root,
            "Invoices/{year}/{month}",
            "Q4_Report.pdf",
            &facts,
            DateSource::Modified,
            &clock,
            &auth,
        )
        .expect("inside root");
        assert_eq!(ok.path, root.join("Invoices/2026/11/Q4_Report.pdf"));

        let err = resolve_destination_path(
            &outside,
            "Invoices/{year}",
            "Q4_Report.pdf",
            &facts,
            DateSource::Modified,
            &clock,
            &auth,
        );
        assert_eq!(err, Err(TemplateError::OutsideApprovedRoots));
    }

    #[test]
    fn r_tpl_08_naming_templates_preserve_extension_without_doubling() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let facts = sample_facts();

        assert_eq!(
            render_filename_template(
                "{date}_{original_name}",
                &facts,
                DateSource::Modified,
                &clock
            )
            .expect("date_name"),
            "2026-11-14_Q4_Report.pdf"
        );
        assert_eq!(
            render_filename_template(
                "{date}_{original_name}.{extension}",
                &facts,
                DateSource::Modified,
                &clock
            )
            .expect("explicit ext"),
            "2026-11-14_Q4_Report.pdf"
        );
        assert_eq!(
            render_filename_template(
                "{category}_{original_name}",
                &facts,
                DateSource::Modified,
                &clock
            )
            .expect("cat_name"),
            "Documents_Q4_Report.pdf"
        );
        assert_eq!(
            render_filename_template(
                "{year}-{month}_{original_name}",
                &facts,
                DateSource::Modified,
                &clock
            )
            .expect("ym_name"),
            "2026-11_Q4_Report.pdf"
        );

        // Extensionless file
        let mut no_ext = sample_facts();
        no_ext.name = "Makefile".into();
        no_ext.stem = "Makefile".into();
        no_ext.extension = "".into();
        assert_eq!(
            render_filename_template(
                "{date}_{original_name}",
                &no_ext,
                DateSource::Modified,
                &clock
            )
            .expect("no ext"),
            "2026-11-14_Makefile"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn r_tpl_prop_output_never_contains_traversal_or_invalid_chars(
            raw_stem in "\\PC{0,80}",
            raw_cat in "\\PC{0,40}",
            raw_src in "\\PC{0,40}",
            raw_ext in "[a-zA-Z0-9./:\\x00]{0,12}",
        ) {
            let (_d, root) = tempdir();
            let auth = PathAuthority::new(vec![ApprovedRoot {
                path: root.clone(),
                kind: RootKind::Destination,
            }]);
            let clock = FixedClock::utc(1_772_942_400_000);
            let mut facts = sample_facts();
            facts.stem = raw_stem;
            facts.category = raw_cat;
            facts.source_folder_name = raw_src;
            facts.extension = raw_ext;

            let sub_res = render_subpath_template(
                "{source_folder}/{category}/{year}-{month}",
                &facts,
                DateSource::Modified,
                &clock,
            );
            prop_assert!(sub_res.is_ok(), "sub_res failed: {:?}", sub_res);
            if let Ok(sub) = &sub_res {
                prop_assert!(!sub.is_empty());
                prop_assert!(sub.len() <= 1024);
                for seg in sub.split('/') {
                    prop_assert!(!seg.is_empty());
                    prop_assert_ne!(seg, ".");
                    prop_assert_ne!(seg, "..");
                    prop_assert!(!seg.contains(':'));
                    prop_assert!(!seg.contains('\0'));
                    prop_assert!(seg.len() <= 255);
                }
            }

            let name_res = render_filename_template(
                "{date}_{original_name}",
                &facts,
                DateSource::Modified,
                &clock,
            );
            prop_assert!(name_res.is_ok(), "name_res failed: {:?}", name_res);
            if let Ok(fname) = &name_res {
                prop_assert!(!fname.is_empty());
                prop_assert!(fname.len() <= 255);
                prop_assert!(!fname.contains('/'));
                prop_assert!(!fname.contains(':'));
                prop_assert!(!fname.contains('\0'));
                prop_assert_ne!(fname, ".");
                prop_assert_ne!(fname, "..");

                if sub_res.is_ok() {
                    if let Ok(dest) = resolve_destination_path(
                        &root,
                        "{source_folder}/{category}/{year}-{month}",
                        fname,
                        &facts,
                        DateSource::Modified,
                        &clock,
                        &auth,
                    ) {
                        prop_assert!(dest.path.starts_with(&root));
                    }
                }
            }
        }
    }
}
