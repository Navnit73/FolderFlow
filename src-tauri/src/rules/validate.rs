//! Rule validation (P3-T04; FR-041, FR-048, FR-050, FR-053, FR-054, FR-072–074).
//!
//! Validates rule drafts at save time and live in the Rule Builder:
//! - Regex syntax and compiled DFA/NFA size limit (`REGEX_SIZE_LIMIT`)
//! - Condition group non-emptiness and max nesting depth (`MAX_CONDITION_DEPTH = 5`)
//! - Action safety (no unsafe/unconfirmed extension changes, no multiple moves, no self-cycles)
//! - Template validity (known variables, no traversal, valid macOS component names)
//! - Destination loop detection (moving into a recursive monitored source folder that would re-trigger)

use std::path::{Component, Path, PathBuf};

use chrono::NaiveDate;
use glob::Pattern;
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::engine::paths::{nfc, validate_component};
use crate::error::{AppError, AppResult};
use crate::rules::conditions::{condition_depth, REGEX_SIZE_LIMIT};
use crate::rules::models::{
    Action, Condition, ConditionNode, DateConditionOp, DateSource, LocationOp, RuleDraft, SizeOp,
    TextOp, MAX_CONDITION_DEPTH,
};
use crate::rules::templates::{
    parse_template_tokens, render_filename_template, render_subpath_template, SampleFile,
};
use crate::utils::clock::FixedClock;

pub const MAX_REGEX_PATTERN_LEN: usize = 512;
pub const MAX_ACTIONS_PER_RULE: usize = 16;

pub const FORBIDDEN_EXECUTABLE_EXTENSIONS: &[&str] = &[
    "app",
    "command",
    "sh",
    "bash",
    "zsh",
    "csh",
    "ksh",
    "pkg",
    "mpkg",
    "dmg",
    "bin",
    "run",
    "dylib",
    "so",
    "exe",
    "bat",
    "cmd",
    "scr",
    "applescript",
    "scpt",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub field: String,
    pub code: String,
    pub message: String,
    pub severity: ValidationSeverity,
}

#[derive(Debug, Clone)]
pub struct MonitoredFolderRef {
    pub id: String,
    pub path: PathBuf,
    pub recursive: bool,
    pub excluded_subfolders: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct ValidationContext {
    pub current_rule_id: Option<String>,
    pub monitored_folders: Vec<MonitoredFolderRef>,
    pub approved_roots: Vec<PathBuf>,
    pub rule_links: std::collections::HashMap<String, Vec<String>>,
}

pub fn validate_rule_draft(draft: &RuleDraft, ctx: &ValidationContext) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    let name = draft.name.trim();
    if name.is_empty() {
        issues.push(err("name", "empty_name", "Rule name cannot be empty."));
    } else if name.chars().count() > 120 {
        issues.push(err(
            "name",
            "name_too_long",
            "Rule name must be 120 characters or fewer.",
        ));
    }

    if condition_depth(&draft.conditions) > MAX_CONDITION_DEPTH {
        issues.push(err(
            "conditions",
            "max_depth_exceeded",
            format!("Conditions cannot be nested more than {MAX_CONDITION_DEPTH} levels deep."),
        ));
    }
    validate_condition_node(&draft.conditions, "conditions", &mut issues);

    if draft.actions.is_empty() {
        issues.push(err(
            "actions",
            "empty_actions",
            "Add at least one action for this rule.",
        ));
    } else if draft.actions.len() > MAX_ACTIONS_PER_RULE {
        issues.push(err(
            "actions",
            "too_many_actions",
            format!("A rule may have at most {MAX_ACTIONS_PER_RULE} actions."),
        ));
    }

    let move_count = draft
        .actions
        .iter()
        .filter(|a| {
            matches!(
                a,
                Action::Move { .. } | Action::MoveIntoDynamicFolder { .. }
            )
        })
        .count();
    if move_count > 1 {
        issues.push(err(
            "actions",
            "multiple_moves",
            "A file can only be moved once per rule.",
        ));
    }

    let sample_facts = SampleFile {
        name: "Sample_Document.pdf".into(),
        category: "Documents".into(),
        source_folder: "Downloads".into(),
        modified_ms: Some(1_772_942_400_000),
        created_ms: Some(1_772_942_400_000),
        added_ms: Some(1_772_942_400_000),
    }
    .to_facts(1_772_942_400_000);
    let clock = FixedClock::utc(1_772_942_400_000);

    for (idx, action) in draft.actions.iter().enumerate() {
        let field = format!("actions.{idx}");
        validate_action(
            action,
            &field,
            draft,
            ctx,
            &sample_facts,
            draft.date_source,
            &clock,
            &mut issues,
        );
    }

    issues
}

fn err(
    field: impl Into<String>,
    code: impl Into<String>,
    message: impl Into<String>,
) -> ValidationIssue {
    ValidationIssue {
        field: field.into(),
        code: code.into(),
        message: message.into(),
        severity: ValidationSeverity::Error,
    }
}

fn warn(
    field: impl Into<String>,
    code: impl Into<String>,
    message: impl Into<String>,
) -> ValidationIssue {
    ValidationIssue {
        field: field.into(),
        code: code.into(),
        message: message.into(),
        severity: ValidationSeverity::Warning,
    }
}

fn validate_condition_node(node: &ConditionNode, path: &str, issues: &mut Vec<ValidationIssue>) {
    match node {
        ConditionNode::All { children } | ConditionNode::Any { children } => {
            if children.is_empty() {
                issues.push(err(
                    path,
                    "empty_group",
                    "Condition group must contain at least one condition.",
                ));
            }
            for (idx, child) in children.iter().enumerate() {
                validate_condition_node(child, &format!("{path}.{idx}"), issues);
            }
        }
        ConditionNode::Not { child } => {
            validate_condition_node(child, &format!("{path}.not"), issues);
        }
        ConditionNode::Leaf { condition } => {
            validate_leaf_condition(condition, path, issues);
        }
    }
}

fn validate_leaf_condition(condition: &Condition, path: &str, issues: &mut Vec<ValidationIssue>) {
    match condition {
        Condition::Text {
            op,
            value,
            case_sensitive,
            ..
        } => {
            if value.trim().is_empty() {
                issues.push(err(path, "empty_value", "Enter a value to match."));
                return;
            }
            if *op == TextOp::MatchesRegex {
                if value.len() > MAX_REGEX_PATTERN_LEN {
                    issues.push(err(
                        path,
                        "regex_too_long",
                        format!("Regular expression must be {MAX_REGEX_PATTERN_LEN} characters or fewer."),
                    ));
                    return;
                }
                if let Err(e) = RegexBuilder::new(value)
                    .case_insensitive(!case_sensitive)
                    .size_limit(REGEX_SIZE_LIMIT)
                    .dfa_size_limit(REGEX_SIZE_LIMIT)
                    .build()
                {
                    issues.push(err(
                        path,
                        "invalid_regex",
                        format!("Invalid or overly complex regular expression: {e}"),
                    ));
                }
            }
        }
        Condition::Category { category_id, .. } => {
            if category_id.trim().is_empty() {
                issues.push(err(path, "empty_category", "Choose a category."));
            }
        }
        Condition::Size {
            op,
            value,
            max_value,
            ..
        } => {
            if *op == SizeOp::Between {
                match max_value {
                    Some(max_v) if *max_v >= *value => {}
                    _ => issues.push(err(
                        path,
                        "invalid_size_range",
                        "Maximum size must be greater than or equal to minimum size.",
                    )),
                }
            }
        }
        Condition::Date { op, .. } => match op {
            DateConditionOp::Before { date } | DateConditionOp::After { date } => {
                if NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").is_err() {
                    issues.push(err(
                        path,
                        "invalid_date",
                        "Date must be in YYYY-MM-DD format.",
                    ));
                }
            }
            DateConditionOp::WithinRange { start, end } => {
                let s = NaiveDate::parse_from_str(start.trim(), "%Y-%m-%d");
                let e = NaiveDate::parse_from_str(end.trim(), "%Y-%m-%d");
                match (s, e) {
                    (Ok(sd), Ok(ed)) if sd <= ed => {}
                    _ => issues.push(err(
                        path,
                        "invalid_date_range",
                        "Start and end dates must be valid YYYY-MM-DD dates with start <= end.",
                    )),
                }
            }
            DateConditionOp::OlderThanDays { days } | DateConditionOp::NewerThanDays { days } => {
                if *days == 0 || *days > 36_500 {
                    issues.push(err(
                        path,
                        "invalid_days",
                        "Days must be between 1 and 36,500.",
                    ));
                }
            }
            DateConditionOp::Today
            | DateConditionOp::Yesterday
            | DateConditionOp::ThisWeek
            | DateConditionOp::ThisMonth
            | DateConditionOp::ThisYear => {}
        },
        Condition::Location { op } => match op {
            LocationOp::SourceFolderEquals { folder } => {
                if folder.trim().is_empty() {
                    issues.push(err(path, "empty_folder", "Select a source folder."));
                }
            }
            LocationOp::SourceFolderContains { text } => {
                if text.trim().is_empty() {
                    issues.push(err(path, "empty_value", "Enter folder text to match."));
                }
            }
            LocationOp::InsideDirectory { path: dir_path } => {
                if dir_path.trim().is_empty() || !Path::new(dir_path.trim()).is_absolute() {
                    issues.push(err(
                        path,
                        "invalid_directory",
                        "Directory path must be an absolute path.",
                    ));
                } else if Path::new(dir_path.trim())
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
                {
                    issues.push(err(
                        path,
                        "invalid_directory",
                        "Directory path must not contain '.' or '..'.",
                    ));
                }
            }
            LocationOp::InsideSubdirectory { subpath } => {
                if let Some(sub) = subpath {
                    if sub.contains("..") {
                        issues.push(err(
                            path,
                            "invalid_subpath",
                            "Subfolder path must not contain '..'.",
                        ));
                    }
                }
            }
        },
        Condition::ContentHash { hash } => {
            let h = hash.trim();
            if h.is_empty() || !h.chars().all(|c| c.is_ascii_hexdigit()) {
                issues.push(err(
                    path,
                    "invalid_hash",
                    "Content hash must be a non-empty hexadecimal string.",
                ));
            }
        }
        Condition::FilenameGlob { pattern, .. } => {
            let p = pattern.trim();
            if p.is_empty() || p.contains('/') {
                issues.push(err(
                    path,
                    "invalid_glob",
                    "Filename glob must be non-empty and cannot contain '/'.",
                ));
            } else if let Err(e) = Pattern::new(p) {
                issues.push(err(
                    path,
                    "invalid_glob",
                    format!("Invalid glob pattern: {e}"),
                ));
            }
        }
        Condition::ExtensionGroup { extensions } => {
            let valid_count = extensions
                .iter()
                .map(|e| e.trim().trim_start_matches('.'))
                .filter(|e| !e.is_empty() && !e.contains('/') && !e.contains(':'))
                .count();
            if valid_count == 0 || valid_count != extensions.len() {
                issues.push(err(
                    path,
                    "invalid_extensions",
                    "Enter at least one valid file extension.",
                ));
            }
        }
        Condition::Hidden { .. }
        | Condition::EmptyFile { .. }
        | Condition::DuplicateStatus { .. } => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_action(
    action: &Action,
    field: &str,
    draft: &RuleDraft,
    ctx: &ValidationContext,
    sample_facts: &crate::rules::models::FileFacts,
    date_source: DateSource,
    clock: &FixedClock,
    issues: &mut Vec<ValidationIssue>,
) {
    match action {
        Action::Move { destination }
        | Action::Copy { destination }
        | Action::CreateDestinationFolder { path: destination } => {
            let dest = destination.trim();
            if dest.is_empty() {
                issues.push(err(
                    field,
                    "empty_destination",
                    "Choose a destination folder.",
                ));
                return;
            }
            if let Some(rel) = dest.strip_prefix('/') {
                if rel.is_empty() {
                    issues.push(err(
                        field,
                        "invalid_destination",
                        "Root '/' cannot be a destination.",
                    ));
                    return;
                }
                if let Err(e) = render_subpath_template(rel, sample_facts, date_source, clock) {
                    issues.push(err(field, "invalid_template", e.to_string()));
                    return;
                }
                let rendered_abs = PathBuf::from(format!(
                    "/{}",
                    render_subpath_template(rel, sample_facts, date_source, clock)
                        .unwrap_or_default()
                ));
                check_approved_roots(&rendered_abs, field, ctx, issues);
                if matches!(action, Action::Move { .. }) {
                    check_destination_loops(&rendered_abs, field, draft, ctx, issues);
                }
            } else {
                match render_subpath_template(dest, sample_facts, date_source, clock) {
                    Ok(rendered_rel) => {
                        if matches!(action, Action::Move { .. }) {
                            check_relative_destination_loops(
                                &rendered_rel,
                                field,
                                draft,
                                ctx,
                                issues,
                            );
                        }
                    }
                    Err(e) => issues.push(err(field, "invalid_template", e.to_string())),
                }
            }
        }
        Action::MoveIntoDynamicFolder {
            root,
            subpath_template,
        } => match render_subpath_template(subpath_template, sample_facts, date_source, clock) {
            Ok(rendered_sub) => {
                if let Some(r) = root.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    let root_path = Path::new(r);
                    if !root_path.is_absolute()
                        || root_path
                            .components()
                            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
                    {
                        issues.push(err(
                            field,
                            "invalid_destination",
                            "Destination root must be a valid absolute path.",
                        ));
                    } else {
                        let full = root_path.join(&rendered_sub);
                        check_approved_roots(&full, field, ctx, issues);
                        check_destination_loops(&full, field, draft, ctx, issues);
                    }
                } else {
                    check_relative_destination_loops(&rendered_sub, field, draft, ctx, issues);
                }
            }
            Err(e) => issues.push(err(field, "invalid_template", e.to_string())),
        },
        Action::Rename { template } => {
            if let Err(e) = render_filename_template(template, sample_facts, date_source, clock) {
                issues.push(err(field, "invalid_template", e.to_string()));
            }
        }
        Action::AddPrefix { prefix: aff } | Action::AddSuffix { suffix: aff } => {
            let trimmed = aff.trim();
            if trimmed.is_empty()
                || trimmed.contains('/')
                || trimmed.contains(':')
                || trimmed.contains('\0')
            {
                issues.push(err(
                    field,
                    "invalid_affix",
                    "Prefix/suffix must be non-empty and cannot contain '/', ':', or NUL.",
                ));
            } else if trimmed.contains('{') || trimmed.contains('}') {
                if let Err(e) = parse_template_tokens(trimmed) {
                    issues.push(err(field, "invalid_template", e.to_string()));
                }
            }
        }
        Action::RemoveText { text, .. } => {
            if text.is_empty() {
                issues.push(err(field, "empty_text", "Enter text to remove."));
            }
        }
        Action::ReplaceText { find, replace, .. } => {
            if find.is_empty() {
                issues.push(err(field, "empty_find", "Enter text to find."));
            }
            if replace.contains('/')
                || replace.contains(':')
                || replace.contains('\0')
                || replace.contains("..")
            {
                issues.push(err(
                    field,
                    "invalid_replacement",
                    "Replacement text cannot contain '/', ':', '..', or NUL.",
                ));
            }
        }
        Action::AddDateToFilename { format, .. } => {
            if let Err(e) = render_filename_template(format, sample_facts, date_source, clock) {
                issues.push(err(field, "invalid_template", e.to_string()));
            }
        }
        Action::ChangeExtension {
            new_extension,
            confirm_unsafe,
        } => {
            let ext = new_extension.trim().trim_start_matches('.').to_lowercase();
            if ext.is_empty()
                || ext.contains('.')
                || validate_component(&ext).is_err()
                || ext.contains(':')
            {
                issues.push(err(
                    field,
                    "invalid_extension",
                    "Enter a valid single extension (without dots or slashes).",
                ));
                return;
            }
            if FORBIDDEN_EXECUTABLE_EXTENSIONS.contains(&ext.as_str()) {
                issues.push(err(
                    field,
                    "unsafe_extension",
                    format!("Changing extensions to '.{ext}' is blocked for safety."),
                ));
                return;
            }
            if !confirm_unsafe {
                issues.push(err(
                    field,
                    "unconfirmed_extension_change",
                    "Confirm that changing the file extension is intentional.",
                ));
            }
        }
        Action::Notify { message } => {
            if message.trim().is_empty() {
                issues.push(err(
                    field,
                    "empty_message",
                    "Notification message cannot be empty.",
                ));
            }
        }
        Action::MarkForReview { reason } => {
            if reason.trim().is_empty() {
                issues.push(err(field, "empty_reason", "Review reason cannot be empty."));
            }
        }
        Action::ApplyAnotherRule { rule_id } => {
            let target = rule_id.trim();
            if target.is_empty() {
                issues.push(err(field, "empty_rule_id", "Choose a target rule."));
            } else if creates_rule_cycle(target, ctx) {
                issues.push(err(
                    field,
                    "self_rule_cycle",
                    "Applying this rule creates a circular rule chain.",
                ));
            }
        }
        Action::Skip | Action::StopProcessing => {}
    }
}

fn creates_rule_cycle(target: &str, ctx: &ValidationContext) -> bool {
    if ctx.current_rule_id.as_deref() == Some(target) {
        return true;
    }
    let mut visited = std::collections::HashSet::new();
    let mut stack = vec![target];
    while let Some(curr) = stack.pop() {
        if ctx.current_rule_id.as_deref() == Some(curr) {
            return true;
        }
        if !visited.insert(curr) {
            return true;
        }
        if let Some(next_ids) = ctx.rule_links.get(curr) {
            for next in next_ids {
                stack.push(next.as_str());
            }
        }
    }
    false
}

fn check_approved_roots(
    dest: &Path,
    field: &str,
    ctx: &ValidationContext,
    issues: &mut Vec<ValidationIssue>,
) {
    if ctx.approved_roots.is_empty() {
        return;
    }
    let dest_nfc = nfc(dest);
    let inside_any = ctx
        .approved_roots
        .iter()
        .any(|r| dest_nfc.starts_with(nfc(r)));
    if !inside_any {
        issues.push(err(
            field,
            "outside_approved_roots",
            "Destination must be inside a monitored folder or approved destination folder.",
        ));
    }
}

fn check_destination_loops(
    dest: &Path,
    field: &str,
    draft: &RuleDraft,
    ctx: &ValidationContext,
    issues: &mut Vec<ValidationIssue>,
) {
    let dest_nfc = nfc(dest);
    for folder in applicable_folders(draft, ctx) {
        let folder_path = nfc(&folder.path);
        if dest_nfc == folder_path {
            issues.push(err(
                field,
                "destination_loop",
                format!(
                    "Destination is the root of monitored folder '{}', which would move files onto themselves.",
                    folder.path.display()
                ),
            ));
            return;
        }
        if folder.recursive && dest_nfc.starts_with(&folder_path) {
            let is_excluded = folder
                .excluded_subfolders
                .iter()
                .any(|ex| dest_nfc.starts_with(nfc(ex)));
            if !is_excluded && !has_subdirectory_guard(&draft.conditions) {
                issues.push(warn(
                    field,
                    "destination_loop",
                    format!(
                        "Destination is inside recursive monitored folder '{}' and may re-trigger this rule unless the subfolder is excluded.",
                        folder.path.display()
                    ),
                ));
            }
        }
    }
}

fn check_relative_destination_loops(
    rendered_rel: &str,
    field: &str,
    draft: &RuleDraft,
    ctx: &ValidationContext,
    issues: &mut Vec<ValidationIssue>,
) {
    for folder in applicable_folders(draft, ctx) {
        if !folder.recursive {
            continue;
        }
        let dest_nfc = nfc(&folder.path.join(rendered_rel));
        let is_excluded = folder
            .excluded_subfolders
            .iter()
            .any(|ex| dest_nfc.starts_with(nfc(ex)));
        if !is_excluded && !has_subdirectory_guard(&draft.conditions) {
            issues.push(warn(
                field,
                "destination_loop",
                format!(
                    "Subfolder '{rendered_rel}' is inside recursive monitored folder '{}' and could re-trigger this rule.",
                    folder.path.display()
                ),
            ));
        }
    }
}

fn applicable_folders<'a>(
    draft: &RuleDraft,
    ctx: &'a ValidationContext,
) -> Vec<&'a MonitoredFolderRef> {
    if draft.source_folders.is_empty() {
        ctx.monitored_folders.iter().collect()
    } else {
        ctx.monitored_folders
            .iter()
            .filter(|f| {
                draft
                    .source_folders
                    .iter()
                    .any(|s| s == &f.id || Path::new(s) == f.path.as_path())
            })
            .collect()
    }
}

fn has_subdirectory_guard(node: &ConditionNode) -> bool {
    match node {
        ConditionNode::Not { child } => matches!(
            child.as_ref(),
            ConditionNode::Leaf {
                condition: Condition::Location {
                    op: LocationOp::InsideSubdirectory { subpath: None }
                        | LocationOp::InsideDirectory { .. }
                }
            }
        ),
        ConditionNode::All { children } => children.iter().any(has_subdirectory_guard),
        ConditionNode::Any { .. } | ConditionNode::Leaf { .. } => false,
    }
}

pub fn ensure_valid_rule(draft: &RuleDraft, ctx: &ValidationContext) -> AppResult<()> {
    let issues = validate_rule_draft(draft, ctx);
    if let Some(first_err) = issues
        .iter()
        .find(|i| i.severity == ValidationSeverity::Error)
    {
        return Err(AppError::validation(
            first_err.field.clone(),
            first_err.message.clone(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::models::{
        Action, Condition, ConditionNode, DateConditionOp, DateField, DateSource, ExecutionMode,
        SizeOp, SizeUnit, TextFieldKind, TextOp,
    };

    fn valid_draft() -> RuleDraft {
        RuleDraft {
            name: "Organize PDFs".into(),
            description: Some("Moves PDFs into Documents".into()),
            enabled: false,
            source_folders: Vec::new(),
            conditions: ConditionNode::All {
                children: vec![ConditionNode::Leaf {
                    condition: Condition::ExtensionGroup {
                        extensions: vec!["pdf".into()],
                    },
                }],
            },
            actions: vec![Action::MoveIntoDynamicFolder {
                root: None,
                subpath_template: "PDFs/{year}".into(),
            }],
            date_source: DateSource::Modified,
            execution_mode: ExecutionMode::FirstMatch,
        }
    }

    #[test]
    fn r_val_01_regex_compile_and_size_limits_enforced() {
        let mut draft = valid_draft();
        draft.conditions = ConditionNode::All {
            children: vec![ConditionNode::Leaf {
                condition: Condition::Text {
                    field: TextFieldKind::Name,
                    op: TextOp::MatchesRegex,
                    value: "[unclosed(".into(),
                    case_sensitive: false,
                },
            }],
        };

        let issues = validate_rule_draft(&draft, &ValidationContext::default());
        assert!(
            issues.iter().any(|i| i.code == "invalid_regex"),
            "expected invalid_regex issue, got: {issues:?}"
        );

        // Pattern exceeding compiled DFA/NFA size limit or max pattern length
        draft.conditions = ConditionNode::All {
            children: vec![ConditionNode::Leaf {
                condition: Condition::Text {
                    field: TextFieldKind::Name,
                    op: TextOp::MatchesRegex,
                    value: "a{1000}{1000}".into(),
                    case_sensitive: false,
                },
            }],
        };
        let issues2 = validate_rule_draft(&draft, &ValidationContext::default());
        assert!(
            issues2
                .iter()
                .any(|i| i.code == "invalid_regex" || i.code == "regex_too_long"),
            "expected regex size limit failure, got: {issues2:?}"
        );

        let huge_pat = "x".repeat(MAX_REGEX_PATTERN_LEN + 10);
        draft.conditions = ConditionNode::All {
            children: vec![ConditionNode::Leaf {
                condition: Condition::Text {
                    field: TextFieldKind::Name,
                    op: TextOp::MatchesRegex,
                    value: huge_pat,
                    case_sensitive: false,
                },
            }],
        };
        let issues3 = validate_rule_draft(&draft, &ValidationContext::default());
        assert!(issues3.iter().any(|i| i.code == "regex_too_long"));
    }

    #[test]
    fn r_val_02_empty_groups_and_excessive_nesting_and_invalid_leaf_values_rejected() {
        let mut draft = valid_draft();
        draft.conditions = ConditionNode::All { children: vec![] };
        let issues = validate_rule_draft(&draft, &ValidationContext::default());
        assert!(issues.iter().any(|i| i.code == "empty_group"));

        // Depth 6 (> 5)
        let mut deep = ConditionNode::Leaf {
            condition: Condition::ExtensionGroup {
                extensions: vec!["pdf".into()],
            },
        };
        for _ in 0..6 {
            deep = ConditionNode::All {
                children: vec![deep],
            };
        }
        draft.conditions = deep;
        let issues_deep = validate_rule_draft(&draft, &ValidationContext::default());
        assert!(issues_deep.iter().any(|i| i.code == "max_depth_exceeded"));

        // Invalid date range & inverted size range
        draft.conditions = ConditionNode::All {
            children: vec![
                ConditionNode::Leaf {
                    condition: Condition::Date {
                        field: DateField::Modified,
                        op: DateConditionOp::WithinRange {
                            start: "2026-12-01".into(),
                            end: "2026-01-01".into(),
                        },
                    },
                },
                ConditionNode::Leaf {
                    condition: Condition::Size {
                        op: SizeOp::Between,
                        value: 500,
                        max_value: Some(100),
                        unit: SizeUnit::Kilobytes,
                    },
                },
            ],
        };
        let issues_ranges = validate_rule_draft(&draft, &ValidationContext::default());
        assert!(issues_ranges.iter().any(|i| i.code == "invalid_date_range"));
        assert!(issues_ranges.iter().any(|i| i.code == "invalid_size_range"));
    }

    #[test]
    fn r_val_03_empty_name_empty_actions_and_multiple_moves_rejected() {
        let mut draft = valid_draft();
        draft.name = "   ".into();
        draft.actions = Vec::new();
        let issues = validate_rule_draft(&draft, &ValidationContext::default());
        assert!(issues.iter().any(|i| i.field == "name"));
        assert!(issues.iter().any(|i| i.code == "empty_actions"));

        draft.name = "Two moves".into();
        draft.actions = vec![
            Action::Move {
                destination: "/Users/alice/Documents/A".into(),
            },
            Action::MoveIntoDynamicFolder {
                root: None,
                subpath_template: "B/{year}".into(),
            },
        ];
        let issues2 = validate_rule_draft(&draft, &ValidationContext::default());
        assert!(issues2.iter().any(|i| i.code == "multiple_moves"));
    }

    #[test]
    fn r_val_04_unsafe_extension_changes_and_bad_templates_rejected() {
        let mut draft = valid_draft();
        // Unconfirmed extension change
        draft.actions = vec![Action::ChangeExtension {
            new_extension: "txt".into(),
            confirm_unsafe: false,
        }];
        let issues = validate_rule_draft(&draft, &ValidationContext::default());
        assert!(issues
            .iter()
            .any(|i| i.code == "unconfirmed_extension_change"));

        // Executable extension change must be blocked even with confirm_unsafe: true
        for forbidden in ["app", "sh", "command", "pkg", "dmg"] {
            draft.actions = vec![Action::ChangeExtension {
                new_extension: forbidden.into(),
                confirm_unsafe: true,
            }];
            let iss = validate_rule_draft(&draft, &ValidationContext::default());
            assert!(
                iss.iter().any(|i| i.code == "unsafe_extension"),
                "failed to reject .{forbidden}"
            );
        }
    }

    #[test]
    fn r_val_05_traversal_unknown_template_vars_and_self_rule_cycle_rejected() {
        let mut draft = valid_draft();
        draft.actions = vec![
            Action::MoveIntoDynamicFolder {
                root: None,
                subpath_template: "Invoices/../Secret/{not_a_var}".into(),
            },
            Action::ApplyAnotherRule {
                rule_id: "rule-self".into(),
            },
        ];
        let ctx = ValidationContext {
            current_rule_id: Some("rule-self".into()),
            ..Default::default()
        };
        let issues = validate_rule_draft(&draft, &ctx);
        assert!(issues.iter().any(|i| i.code == "invalid_template"));
        assert!(issues.iter().any(|i| i.code == "self_rule_cycle"));
    }

    #[test]
    fn r_val_06_destination_loop_inside_monitored_source_folder_is_flagged() {
        let mut draft = valid_draft();
        draft.source_folders = vec!["f-dl".into()];

        // 1. Moving directly to the monitored source folder itself (circular no-op loop)
        draft.actions = vec![Action::Move {
            destination: "/Users/alice/Downloads".into(),
        }];
        let ctx = ValidationContext {
            current_rule_id: None,
            monitored_folders: vec![MonitoredFolderRef {
                id: "f-dl".into(),
                path: PathBuf::from("/Users/alice/Downloads"),
                recursive: true,
                excluded_subfolders: Vec::new(),
            }],
            approved_roots: vec![PathBuf::from("/Users/alice/Downloads")],
            ..Default::default()
        };
        let issues1 = validate_rule_draft(&draft, &ctx);
        assert!(
            issues1.iter().any(|i| i.code == "destination_loop"),
            "expected destination_loop for same folder, got: {issues1:?}"
        );

        // 2. Moving into a subfolder of a recursive monitored source folder when the rule
        //    still matches files in that subfolder (FR-054)
        draft.actions = vec![Action::MoveIntoDynamicFolder {
            root: None,
            subpath_template: "PDFs/{year}".into(),
        }];
        let issues2 = validate_rule_draft(&draft, &ctx);
        assert!(
            issues2.iter().any(|i| i.code == "destination_loop"),
            "expected destination_loop for recursive subfolder re-trigger, got: {issues2:?}"
        );
    }

    #[test]
    fn bug_017_not_inside_subdirectory_in_any_branch_does_not_suppress_destination_loop_warning() {
        let mut draft = valid_draft();
        draft.source_folders = vec!["f-dl".into()];
        draft.conditions = ConditionNode::Any {
            children: vec![
                ConditionNode::Leaf {
                    condition: Condition::ExtensionGroup {
                        extensions: vec!["pdf".into()],
                    },
                },
                ConditionNode::Not {
                    child: Box::new(ConditionNode::Leaf {
                        condition: Condition::Location {
                            op: LocationOp::InsideSubdirectory { subpath: None },
                        },
                    }),
                },
            ],
        };
        draft.actions = vec![Action::MoveIntoDynamicFolder {
            root: None,
            subpath_template: "PDFs/{year}".into(),
        }];
        let ctx = ValidationContext {
            current_rule_id: None,
            monitored_folders: vec![MonitoredFolderRef {
                id: "f-dl".into(),
                path: PathBuf::from("/Users/alice/Downloads"),
                recursive: true,
                excluded_subfolders: Vec::new(),
            }],
            approved_roots: vec![PathBuf::from("/Users/alice/Downloads")],
            ..Default::default()
        };
        let issues = validate_rule_draft(&draft, &ctx);
        assert!(
            issues.iter().any(|i| i.code == "destination_loop"),
            "expected destination_loop warning when NOT(InsideSubdirectory) is inside an ANY branch, got: {issues:?}"
        );
    }
}
