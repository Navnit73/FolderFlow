//! Pure condition and boolean group evaluators (P3-T02, P3-T03; FR-041–047).
//!
//! No filesystem or database I/O occurs here. All file attributes come from
//! `FileFacts` and all current-time / local-timezone calculations go through `Clock`.

use std::path::Path;

use chrono::{Datelike, Days, NaiveDate};
use glob::{MatchOptions, Pattern};
use regex::RegexBuilder;
use unicode_normalization::UnicodeNormalization;

use crate::rules::models::{
    Condition, ConditionNode, DateConditionOp, DateField, FileFacts, LocationOp, SizeOp,
    TextFieldKind, TextOp, MAX_CONDITION_DEPTH,
};
use crate::utils::clock::Clock;

pub const REGEX_SIZE_LIMIT: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupEvalResult {
    pub matched: bool,
    pub evaluated_leaves: usize,
    pub matched_leaves: Vec<String>,
}

/// Computes the group nesting depth of a `ConditionNode` tree.
/// A `Leaf` has depth 0; a single group (`All`/`Any`/`Not`) of leaves has depth 1.
pub fn condition_depth(node: &ConditionNode) -> usize {
    match node {
        ConditionNode::Leaf { .. } => 0,
        ConditionNode::Not { child } => 1 + condition_depth(child),
        ConditionNode::All { children } | ConditionNode::Any { children } => {
            1 + children.iter().map(condition_depth).max().unwrap_or(0)
        }
    }
}

/// Evaluates a single `Condition` against `facts` and `clock`.
pub fn evaluate_condition(cond: &Condition, facts: &FileFacts, clock: &dyn Clock) -> bool {
    match cond {
        Condition::Text {
            field,
            op,
            value,
            case_sensitive,
        } => eval_text(*field, *op, value, *case_sensitive, facts),
        Condition::Category {
            category_id,
            negate,
        } => {
            let eq = facts
                .category
                .trim()
                .eq_ignore_ascii_case(category_id.trim());
            if *negate {
                !eq
            } else {
                eq
            }
        }
        Condition::Size {
            op,
            value,
            max_value,
            unit,
        } => {
            let lo = unit.to_bytes(*value);
            match op {
                SizeOp::GreaterThan => facts.size > lo,
                SizeOp::LessThan => facts.size < lo,
                SizeOp::Equals => facts.size == lo,
                SizeOp::Between => {
                    let hi = unit.to_bytes(max_value.unwrap_or(*value));
                    let (min_b, max_b) = (lo.min(hi), lo.max(hi));
                    facts.size >= min_b && facts.size <= max_b
                }
            }
        }
        Condition::Date { field, op } => eval_date(*field, op, facts, clock),
        Condition::Location { op } => eval_location(op, facts),
        Condition::Hidden { is_hidden } => facts.is_hidden == *is_hidden,
        Condition::EmptyFile { is_empty } => (facts.size == 0) == *is_empty,
        Condition::DuplicateStatus { is_duplicate } => facts.is_duplicate == *is_duplicate,
        Condition::ContentHash { hash } => facts
            .content_hash
            .as_ref()
            .is_some_and(|h| !hash.trim().is_empty() && h.trim().eq_ignore_ascii_case(hash.trim())),
        Condition::FilenameGlob {
            pattern,
            case_sensitive,
        } => {
            let pat_nfc = nfc_str(pattern.trim());
            if pat_nfc.is_empty() {
                return false;
            }
            let Ok(compiled) = Pattern::new(&pat_nfc) else {
                return false;
            };
            let name_nfc = nfc_str(&facts.name);
            let opts = MatchOptions {
                case_sensitive: *case_sensitive,
                require_literal_separator: true,
                require_literal_leading_dot: false,
            };
            compiled.matches_with(&name_nfc, opts)
        }
        Condition::ExtensionGroup { extensions } => {
            if extensions.is_empty() {
                return false;
            }
            let file_ext = facts
                .extension
                .trim()
                .trim_start_matches('.')
                .to_lowercase();
            let file_name_lower = nfc_str(&facts.name).to_lowercase();
            extensions.iter().any(|ext| {
                let candidate = nfc_str(ext.trim()).trim_start_matches('.').to_lowercase();
                if candidate.is_empty() {
                    return false;
                }
                if candidate.contains('.') {
                    file_name_lower.ends_with(&format!(".{candidate}"))
                } else {
                    file_ext == candidate
                }
            })
        }
    }
}

/// Evaluates a `ConditionNode` tree with short-circuiting and depth enforcement.
pub fn evaluate_node(
    node: &ConditionNode,
    facts: &FileFacts,
    clock: &dyn Clock,
) -> GroupEvalResult {
    if condition_depth(node) > MAX_CONDITION_DEPTH {
        return GroupEvalResult {
            matched: false,
            evaluated_leaves: 0,
            matched_leaves: Vec::new(),
        };
    }
    eval_node_inner(node, facts, clock, "root")
}

fn eval_node_inner(
    node: &ConditionNode,
    facts: &FileFacts,
    clock: &dyn Clock,
    path: &str,
) -> GroupEvalResult {
    match node {
        ConditionNode::Leaf { condition } => {
            let matched = evaluate_condition(condition, facts, clock);
            GroupEvalResult {
                matched,
                evaluated_leaves: 1,
                matched_leaves: if matched {
                    vec![path.to_owned()]
                } else {
                    Vec::new()
                },
            }
        }
        ConditionNode::Not { child } => {
            let sub = eval_node_inner(child, facts, clock, &format!("{path}.not"));
            GroupEvalResult {
                matched: !sub.matched,
                evaluated_leaves: sub.evaluated_leaves,
                matched_leaves: if !sub.matched {
                    vec![path.to_owned()]
                } else {
                    Vec::new()
                },
            }
        }
        ConditionNode::All { children } => {
            let mut evaluated_leaves = 0;
            let mut matched_leaves = Vec::new();
            for (idx, child) in children.iter().enumerate() {
                let sub = eval_node_inner(child, facts, clock, &format!("{path}.{idx}"));
                evaluated_leaves += sub.evaluated_leaves;
                if !sub.matched {
                    return GroupEvalResult {
                        matched: false,
                        evaluated_leaves,
                        matched_leaves: Vec::new(),
                    };
                }
                matched_leaves.extend(sub.matched_leaves);
            }
            GroupEvalResult {
                matched: true,
                evaluated_leaves,
                matched_leaves,
            }
        }
        ConditionNode::Any { children } => {
            let mut evaluated_leaves = 0;
            for (idx, child) in children.iter().enumerate() {
                let sub = eval_node_inner(child, facts, clock, &format!("{path}.{idx}"));
                evaluated_leaves += sub.evaluated_leaves;
                if sub.matched {
                    return GroupEvalResult {
                        matched: true,
                        evaluated_leaves,
                        matched_leaves: sub.matched_leaves,
                    };
                }
            }
            GroupEvalResult {
                matched: false,
                evaluated_leaves,
                matched_leaves: Vec::new(),
            }
        }
    }
}

fn nfc_str(s: &str) -> String {
    s.nfc().collect()
}

fn eval_text(
    field: TextFieldKind,
    op: TextOp,
    value: &str,
    case_sensitive: bool,
    facts: &FileFacts,
) -> bool {
    let raw_target: String = match field {
        TextFieldKind::Name => facts.name.clone(),
        TextFieldKind::Stem => facts.stem.clone(),
        TextFieldKind::Extension => facts.extension.trim_start_matches('.').to_owned(),
        TextFieldKind::MimeType => facts.mime.clone().unwrap_or_default(),
        TextFieldKind::FullPath => facts.path.clone(),
        TextFieldKind::ParentFolder => facts.parent_folder_name(),
    };
    let raw_pat = match field {
        TextFieldKind::Extension => value.trim().trim_start_matches('.'),
        _ => value,
    };

    let target_nfc = nfc_str(&raw_target);
    let pat_nfc = nfc_str(raw_pat);

    if op == TextOp::MatchesRegex {
        if pat_nfc.is_empty() {
            return false;
        }
        let Ok(re) = RegexBuilder::new(&pat_nfc)
            .case_insensitive(!case_sensitive)
            .size_limit(REGEX_SIZE_LIMIT)
            .dfa_size_limit(REGEX_SIZE_LIMIT)
            .build()
        else {
            return false;
        };
        return re.is_match(&target_nfc);
    }

    let (lhs, rhs) = if case_sensitive {
        (target_nfc, pat_nfc)
    } else {
        (target_nfc.to_lowercase(), pat_nfc.to_lowercase())
    };

    match op {
        TextOp::Contains => lhs.contains(&rhs),
        TextOp::NotContains => !lhs.contains(&rhs),
        TextOp::StartsWith => lhs.starts_with(&rhs),
        TextOp::EndsWith => lhs.ends_with(&rhs),
        TextOp::Equals => lhs == rhs,
        TextOp::NotEquals => lhs != rhs,
        TextOp::MatchesRegex => unreachable!(),
    }
}

fn eval_date(field: DateField, op: &DateConditionOp, facts: &FileFacts, clock: &dyn Clock) -> bool {
    let file_ms = match field {
        DateField::Created => facts.created_ms.unwrap_or(facts.modified_ms),
        DateField::Modified => facts.modified_ms,
        DateField::Added => facts.added_ms.unwrap_or(facts.modified_ms),
    };

    let file_local = clock.to_local(file_ms);
    let file_date = file_local.date_naive();
    let now_ms = clock.now_ms();
    let now_local = clock.to_local(now_ms);
    let today = now_local.date_naive();

    match op {
        DateConditionOp::Before { date } => parse_ymd(date).is_some_and(|d| file_date < d),
        DateConditionOp::After { date } => parse_ymd(date).is_some_and(|d| file_date > d),
        DateConditionOp::WithinRange { start, end } => {
            let Some(s) = parse_ymd(start) else {
                return false;
            };
            let Some(e) = parse_ymd(end) else {
                return false;
            };
            let (lo, hi) = (s.min(e), s.max(e));
            file_date >= lo && file_date <= hi
        }
        DateConditionOp::OlderThanDays { days } => {
            let Some(cutoff_date) = today.checked_sub_days(Days::new(u64::from(*days))) else {
                return false;
            };
            (file_date, file_local.time()) < (cutoff_date, now_local.time())
        }
        DateConditionOp::NewerThanDays { days } => {
            let Some(cutoff_date) = today.checked_sub_days(Days::new(u64::from(*days))) else {
                return false;
            };
            file_ms <= now_ms && (file_date, file_local.time()) >= (cutoff_date, now_local.time())
        }
        DateConditionOp::Today => file_date == today,
        DateConditionOp::Yesterday => today.pred_opt() == Some(file_date),
        DateConditionOp::ThisWeek => {
            file_date.iso_week() == today.iso_week() && (file_date - today).num_days().abs() < 7
        }
        DateConditionOp::ThisMonth => {
            file_date.year() == today.year() && file_date.month() == today.month()
        }
        DateConditionOp::ThisYear => file_date.year() == today.year(),
    }
}

fn parse_ymd(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

fn eval_location(op: &LocationOp, facts: &FileFacts) -> bool {
    match op {
        LocationOp::SourceFolderEquals { folder } => {
            let f_nfc = nfc_str(folder.trim());
            if f_nfc.is_empty() {
                return false;
            }
            facts.source_folder_id == f_nfc
                || nfc_str(&facts.source_folder_path) == f_nfc
                || nfc_str(&facts.source_folder_name).eq_ignore_ascii_case(&f_nfc)
        }
        LocationOp::SourceFolderContains { text } => {
            let t_lower = nfc_str(text.trim()).to_lowercase();
            if t_lower.is_empty() {
                return false;
            }
            nfc_str(&facts.source_folder_path)
                .to_lowercase()
                .contains(&t_lower)
                || nfc_str(&facts.source_folder_name)
                    .to_lowercase()
                    .contains(&t_lower)
        }
        LocationOp::InsideDirectory { path } => {
            let target_str = nfc_str(path.trim());
            if target_str.is_empty() {
                return false;
            }
            let file_nfc = nfc_str(&facts.path);
            let Some(parent) = Path::new(&file_nfc).parent() else {
                return false;
            };
            parent.starts_with(Path::new(&target_str))
        }
        LocationOp::InsideSubdirectory { subpath } => {
            let file_nfc = nfc_str(&facts.path);
            let src_nfc = nfc_str(&facts.source_folder_path);
            let Some(parent) = Path::new(&file_nfc).parent() else {
                return false;
            };
            let src_path = Path::new(&src_nfc);
            if parent == src_path || !parent.starts_with(src_path) {
                return false;
            }
            let Ok(rel) = parent.strip_prefix(src_path) else {
                return false;
            };
            if let Some(sub) = subpath
                .as_ref()
                .map(|s| nfc_str(s.trim()))
                .filter(|s| !s.is_empty())
            {
                let sub_clean = sub.trim_matches('/');
                rel.starts_with(Path::new(sub_clean))
                    || rel
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&sub_clean.to_lowercase())
            } else {
                !rel.as_os_str().is_empty()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::models::{
        DateConditionOp, DateField, LocationOp, SizeOp, SizeUnit, TextFieldKind, TextOp,
    };
    use crate::utils::clock::FixedClock;
    use chrono::TimeZone;
    use proptest::prelude::*;
    use unicode_normalization::UnicodeNormalization;

    fn sample_facts() -> FileFacts {
        FileFacts {
            path: "/Users/alice/Downloads/Invoices/INV-2026-001.pdf".into(),
            name: "INV-2026-001.pdf".into(),
            stem: "INV-2026-001".into(),
            extension: "pdf".into(),
            mime: Some("application/pdf".into()),
            category: "documents".into(),
            size: 250_000,
            created_ms: Some(1_772_942_400_000), // 2026-03-08T04:00:00Z
            modified_ms: 1_772_942_400_000,      // 2026-03-08T04:00:00Z
            added_ms: Some(1_772_946_000_000),   // 2026-03-08T05:00:00Z
            is_hidden: false,
            is_duplicate: false,
            content_hash: Some("a1b2c3d4e5f6".into()),
            source_folder_id: "folder-dl".into(),
            source_folder_path: "/Users/alice/Downloads".into(),
            source_folder_name: "Downloads".into(),
        }
    }

    fn utc_ms(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> i64 {
        chrono::Utc
            .with_ymd_and_hms(y, m, d, hh, mm, ss)
            .single()
            .expect("valid utc datetime")
            .timestamp_millis()
    }

    #[test]
    fn r_cond_01_text_contains_and_not_contains_case_insensitive_default() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let facts = sample_facts();

        let contains_ci = Condition::Text {
            field: TextFieldKind::Name,
            op: TextOp::Contains,
            value: "inv-2026".into(),
            case_sensitive: false,
        };
        assert!(evaluate_condition(&contains_ci, &facts, &clock));

        let contains_cs = Condition::Text {
            field: TextFieldKind::Name,
            op: TextOp::Contains,
            value: "inv-2026".into(),
            case_sensitive: true,
        };
        assert!(!evaluate_condition(&contains_cs, &facts, &clock));

        let not_contains = Condition::Text {
            field: TextFieldKind::Name,
            op: TextOp::NotContains,
            value: "receipt".into(),
            case_sensitive: false,
        };
        assert!(evaluate_condition(&not_contains, &facts, &clock));
    }

    #[test]
    fn r_cond_02_text_starts_ends_equals_not_equals_and_nfc_normalization() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let mut facts = sample_facts();

        assert!(evaluate_condition(
            &Condition::Text {
                field: TextFieldKind::Stem,
                op: TextOp::StartsWith,
                value: "inv-".into(),
                case_sensitive: false,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Text {
                field: TextFieldKind::Name,
                op: TextOp::EndsWith,
                value: ".PDF".into(),
                case_sensitive: false,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Text {
                field: TextFieldKind::Extension,
                op: TextOp::Equals,
                value: ".pdf".into(),
                case_sensitive: false,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Text {
                field: TextFieldKind::MimeType,
                op: TextOp::Equals,
                value: "application/pdf".into(),
                case_sensitive: false,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Text {
                field: TextFieldKind::ParentFolder,
                op: TextOp::Equals,
                value: "invoices".into(),
                case_sensitive: false,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Text {
                field: TextFieldKind::FullPath,
                op: TextOp::NotEquals,
                value: "/other".into(),
                case_sensitive: false,
            },
            &facts,
            &clock
        ));

        // Unicode NFC vs NFD equivalence
        facts.name = "Café_Résumé.pdf".nfd().collect();
        assert!(evaluate_condition(
            &Condition::Text {
                field: TextFieldKind::Name,
                op: TextOp::Contains,
                value: "café_résumé".nfc().collect(),
                case_sensitive: false,
            },
            &facts,
            &clock
        ));
    }

    #[test]
    fn r_cond_03_text_matches_regex_with_case_toggle() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let facts = sample_facts();

        let re_ci = Condition::Text {
            field: TextFieldKind::Name,
            op: TextOp::MatchesRegex,
            value: r"^inv-\d{4}-\d{3}\.pdf$".into(),
            case_sensitive: false,
        };
        assert!(evaluate_condition(&re_ci, &facts, &clock));

        let re_cs = Condition::Text {
            field: TextFieldKind::Name,
            op: TextOp::MatchesRegex,
            value: r"^inv-\d{4}-\d{3}\.pdf$".into(),
            case_sensitive: true,
        };
        assert!(!evaluate_condition(&re_cs, &facts, &clock));

        let bad_re = Condition::Text {
            field: TextFieldKind::Name,
            op: TextOp::MatchesRegex,
            value: "[unclosed".into(),
            case_sensitive: false,
        };
        assert!(!evaluate_condition(&bad_re, &facts, &clock));
    }

    #[test]
    fn r_cond_04_category_and_hidden_and_empty() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let mut facts = sample_facts();

        assert!(evaluate_condition(
            &Condition::Category {
                category_id: "documents".into(),
                negate: false,
            },
            &facts,
            &clock
        ));
        assert!(!evaluate_condition(
            &Condition::Category {
                category_id: "documents".into(),
                negate: true,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Hidden { is_hidden: false },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::EmptyFile { is_empty: false },
            &facts,
            &clock
        ));

        facts.size = 0;
        facts.is_hidden = true;
        assert!(evaluate_condition(
            &Condition::Hidden { is_hidden: true },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::EmptyFile { is_empty: true },
            &facts,
            &clock
        ));
    }

    #[test]
    fn r_cond_05_size_operators_and_decimal_units() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let mut facts = sample_facts();
        facts.size = 250_000; // 250 KB (decimal: 1 KB = 1000 B per FR-044)

        assert!(evaluate_condition(
            &Condition::Size {
                op: SizeOp::Equals,
                value: 250,
                max_value: None,
                unit: SizeUnit::Kilobytes,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Size {
                op: SizeOp::GreaterThan,
                value: 200,
                max_value: None,
                unit: SizeUnit::Kilobytes,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Size {
                op: SizeOp::LessThan,
                value: 1,
                max_value: None,
                unit: SizeUnit::Megabytes,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Size {
                op: SizeOp::Between,
                value: 100,
                max_value: Some(300),
                unit: SizeUnit::Kilobytes,
            },
            &facts,
            &clock
        ));

        facts.size = 2_000_000_000; // 2 GB
        assert!(evaluate_condition(
            &Condition::Size {
                op: SizeOp::GreaterThan,
                value: 1,
                max_value: None,
                unit: SizeUnit::Gigabytes,
            },
            &facts,
            &clock
        ));
    }

    #[test]
    fn r_cond_06_date_before_after_and_range_in_local_tz() {
        // File modified at 2026-06-15T10:00:00Z
        let mut facts = sample_facts();
        facts.modified_ms = utc_ms(2026, 6, 15, 10, 0, 0);
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));

        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::Before {
                    date: "2026-06-16".into()
                },
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::After {
                    date: "2026-06-14".into()
                },
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::WithinRange {
                    start: "2026-06-01".into(),
                    end: "2026-06-30".into(),
                },
            },
            &facts,
            &clock
        ));
    }

    #[test]
    fn r_cond_07_date_older_and_newer_than_days() {
        let now = utc_ms(2026, 11, 14, 12, 0, 0);
        let clock = FixedClock::utc(now);
        let mut facts = sample_facts();
        // 35 days before now
        facts.modified_ms = now - 35 * 86_400_000;

        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::OlderThanDays { days: 30 },
            },
            &facts,
            &clock
        ));
        assert!(!evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::NewerThanDays { days: 30 },
            },
            &facts,
            &clock
        ));

        // 5 days before now
        facts.modified_ms = now - 5 * 86_400_000;
        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::NewerThanDays { days: 7 },
            },
            &facts,
            &clock
        ));
    }

    #[test]
    fn r_cond_08_date_today_and_yesterday_respect_local_timezone() {
        // Now is 2026-03-08T15:00:00Z (10:00 AM in UTC-5).
        // File modified at 2026-03-08T03:30:00Z:
        //   In UTC, that is 2026-03-08 (Today).
        //   In UTC-5, that is 2026-03-07 22:30:00 (Yesterday!).
        let now = utc_ms(2026, 3, 8, 15, 0, 0);
        let mut facts = sample_facts();
        facts.modified_ms = utc_ms(2026, 3, 8, 3, 30, 0);

        let ny_clock = FixedClock::with_offset_mins(now, -300); // UTC-5
        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::Yesterday,
            },
            &facts,
            &ny_clock
        ));
        assert!(!evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::Today,
            },
            &facts,
            &ny_clock
        ));

        let utc_clock = FixedClock::utc(now);
        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::Today,
            },
            &facts,
            &utc_clock
        ));
    }

    #[test]
    fn r_cond_09_date_this_week_month_year() {
        // 2026-11-14 is Saturday (ISO week 46 of 2026).
        let now = utc_ms(2026, 11, 14, 12, 0, 0);
        let clock = FixedClock::utc(now);
        let mut facts = sample_facts();

        // Monday of the same ISO week: 2026-11-09
        facts.modified_ms = utc_ms(2026, 11, 9, 9, 0, 0);
        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::ThisWeek,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::ThisMonth,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::ThisYear,
            },
            &facts,
            &clock
        ));

        // Earlier in November (previous week): 2026-11-02
        facts.modified_ms = utc_ms(2026, 11, 2, 9, 0, 0);
        assert!(!evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::ThisWeek,
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::ThisMonth,
            },
            &facts,
            &clock
        ));
    }

    #[test]
    fn r_cond_10_date_dst_spring_forward_and_fall_back_boundaries() {
        // America/New_York springs forward on 2026-03-08 at 07:00:00 UTC (02:00 EST -> 03:00 EDT).
        // Now is Sunday 2026-03-08 16:00:00 UTC = 12:00:00 EDT (UTC-4).
        let transition = utc_ms(2026, 3, 8, 7, 0, 0);
        let now = utc_ms(2026, 3, 8, 16, 0, 0);
        let dst_clock = FixedClock::with_dst(now, transition, -300, -240);

        let mut facts = sample_facts();
        // Saturday 2026-03-07 16:30:00 UTC = Saturday 11:30:00 EST (UTC-5).
        // Only 23.5 real hours elapsed, but in local wall-clock time it is > 1 day before Sunday 12:00!
        facts.modified_ms = utc_ms(2026, 3, 7, 16, 30, 0);

        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::OlderThanDays { days: 1 },
            },
            &facts,
            &dst_clock
        ));
        assert!(evaluate_condition(
            &Condition::Date {
                field: DateField::Modified,
                op: DateConditionOp::Yesterday,
            },
            &facts,
            &dst_clock
        ));
    }

    #[test]
    fn r_cond_11_location_source_folder_equals_and_contains() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let facts = sample_facts();

        assert!(evaluate_condition(
            &Condition::Location {
                op: LocationOp::SourceFolderEquals {
                    folder: "folder-dl".into()
                },
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Location {
                op: LocationOp::SourceFolderEquals {
                    folder: "/Users/alice/Downloads".into()
                },
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Location {
                op: LocationOp::SourceFolderContains {
                    text: "downloads".into()
                },
            },
            &facts,
            &clock
        ));
    }

    #[test]
    fn r_cond_12_location_inside_directory_and_inside_subdirectory() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let mut facts = sample_facts();

        // Inside /Users/alice/Downloads/Invoices
        assert!(evaluate_condition(
            &Condition::Location {
                op: LocationOp::InsideDirectory {
                    path: "/Users/alice/Downloads/Invoices".into()
                },
            },
            &facts,
            &clock
        ));
        // Sibling prefix must NOT match
        assert!(!evaluate_condition(
            &Condition::Location {
                op: LocationOp::InsideDirectory {
                    path: "/Users/alice/Downloads/Invoices2".into()
                },
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Location {
                op: LocationOp::InsideSubdirectory { subpath: None },
            },
            &facts,
            &clock
        ));
        assert!(evaluate_condition(
            &Condition::Location {
                op: LocationOp::InsideSubdirectory {
                    subpath: Some("Invoices".into())
                },
            },
            &facts,
            &clock
        ));

        // Direct child of source folder is NOT inside a subdirectory
        facts.path = "/Users/alice/Downloads/INV-2026-001.pdf".into();
        assert!(!evaluate_condition(
            &Condition::Location {
                op: LocationOp::InsideSubdirectory { subpath: None },
            },
            &facts,
            &clock
        ));
    }

    #[test]
    fn r_cond_13_advanced_content_hash_and_duplicate_status() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let mut facts = sample_facts();

        assert!(evaluate_condition(
            &Condition::ContentHash {
                hash: "A1B2C3D4E5F6".into()
            },
            &facts,
            &clock
        ));
        assert!(!evaluate_condition(
            &Condition::DuplicateStatus { is_duplicate: true },
            &facts,
            &clock
        ));
        facts.is_duplicate = true;
        assert!(evaluate_condition(
            &Condition::DuplicateStatus { is_duplicate: true },
            &facts,
            &clock
        ));
    }

    #[test]
    fn r_cond_14_advanced_filename_glob() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let mut facts = sample_facts();
        facts.name = "Screenshot 2026-11-14 at 10.05.22.png".into();

        assert!(evaluate_condition(
            &Condition::FilenameGlob {
                pattern: "screenshot*.png".into(),
                case_sensitive: false,
            },
            &facts,
            &clock
        ));
        assert!(!evaluate_condition(
            &Condition::FilenameGlob {
                pattern: "screenshot*.png".into(),
                case_sensitive: true,
            },
            &facts,
            &clock
        ));
    }

    #[test]
    fn r_cond_15_advanced_extension_group_including_compound_extensions() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let mut facts = sample_facts();

        assert!(evaluate_condition(
            &Condition::ExtensionGroup {
                extensions: vec![".PDF".into(), "docx".into()],
            },
            &facts,
            &clock
        ));

        facts.name = "backup-2026.tar.gz".into();
        facts.extension = "gz".into();
        assert!(evaluate_condition(
            &Condition::ExtensionGroup {
                extensions: vec!["tar.gz".into()],
            },
            &facts,
            &clock
        ));
    }

    #[test]
    fn r_grp_01_all_and_any_and_not_truth_tables() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let facts = sample_facts();

        let t_leaf = ConditionNode::Leaf {
            condition: Condition::Category {
                category_id: "documents".into(),
                negate: false,
            },
        };
        let f_leaf = ConditionNode::Leaf {
            condition: Condition::Category {
                category_id: "images".into(),
                negate: false,
            },
        };

        assert!(
            evaluate_node(
                &ConditionNode::All {
                    children: vec![t_leaf.clone(), t_leaf.clone()]
                },
                &facts,
                &clock
            )
            .matched
        );
        assert!(
            !evaluate_node(
                &ConditionNode::All {
                    children: vec![t_leaf.clone(), f_leaf.clone()]
                },
                &facts,
                &clock
            )
            .matched
        );
        assert!(
            evaluate_node(
                &ConditionNode::Any {
                    children: vec![f_leaf.clone(), t_leaf.clone()]
                },
                &facts,
                &clock
            )
            .matched
        );
        assert!(
            evaluate_node(
                &ConditionNode::Not {
                    child: Box::new(f_leaf)
                },
                &facts,
                &clock
            )
            .matched
        );
    }

    #[test]
    fn r_grp_02_empty_all_is_true_and_empty_any_is_false() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let facts = sample_facts();

        assert!(evaluate_node(&ConditionNode::All { children: vec![] }, &facts, &clock).matched);
        assert!(!evaluate_node(&ConditionNode::Any { children: vec![] }, &facts, &clock).matched);
    }

    #[test]
    fn r_grp_03_short_circuit_evaluation() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let facts = sample_facts();

        let t_leaf = ConditionNode::Leaf {
            condition: Condition::Category {
                category_id: "documents".into(),
                negate: false,
            },
        };
        let f_leaf = ConditionNode::Leaf {
            condition: Condition::Category {
                category_id: "images".into(),
                negate: false,
            },
        };

        // All: first child is false -> second and third children must not be evaluated
        let all_res = evaluate_node(
            &ConditionNode::All {
                children: vec![f_leaf.clone(), t_leaf.clone(), t_leaf.clone()],
            },
            &facts,
            &clock,
        );
        assert!(!all_res.matched);
        assert_eq!(all_res.evaluated_leaves, 1);

        // Any: first child is true -> second and third children must not be evaluated
        let any_res = evaluate_node(
            &ConditionNode::Any {
                children: vec![t_leaf, f_leaf.clone(), f_leaf],
            },
            &facts,
            &clock,
        );
        assert!(any_res.matched);
        assert_eq!(any_res.evaluated_leaves, 1);
    }

    #[test]
    fn r_grp_04_nesting_depth_up_to_5_evaluates_and_exceeding_5_is_rejected() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let facts = sample_facts();

        let leaf = ConditionNode::Leaf {
            condition: Condition::Category {
                category_id: "documents".into(),
                negate: false,
            },
        };

        let mut depth_5 = leaf.clone();
        for _ in 0..MAX_CONDITION_DEPTH {
            depth_5 = ConditionNode::All {
                children: vec![depth_5],
            };
        }
        assert_eq!(condition_depth(&depth_5), MAX_CONDITION_DEPTH);
        assert!(evaluate_node(&depth_5, &facts, &clock).matched);

        let depth_6 = ConditionNode::All {
            children: vec![depth_5],
        };
        assert_eq!(condition_depth(&depth_6), MAX_CONDITION_DEPTH + 1);
        assert!(!evaluate_node(&depth_6, &facts, &clock).matched);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn r_cond_prop_size_and_text_invariants(
            file_size in 0u64..10_000_000,
            a in 0u64..5_000,
            b in 0u64..5_000,
            sub in "[a-zA-Z0-9_-]{1,8}",
            prefix in "[a-zA-Z0-9]{0,5}",
            suffix in "[a-zA-Z0-9]{0,5}",
        ) {
            let clock = FixedClock::utc(1_772_942_400_000);
            let mut facts = sample_facts();
            facts.size = file_size;
            facts.name = format!("{prefix}{sub}{suffix}.pdf");

            let eq = evaluate_condition(&Condition::Text {
                field: TextFieldKind::Name,
                op: TextOp::Equals,
                value: facts.name.clone(),
                case_sensitive: false,
            }, &facts, &clock);
            let neq = evaluate_condition(&Condition::Text {
                field: TextFieldKind::Name,
                op: TextOp::NotEquals,
                value: facts.name.clone(),
                case_sensitive: false,
            }, &facts, &clock);
            prop_assert!(eq && !neq);

            let contains = evaluate_condition(&Condition::Text {
                field: TextFieldKind::Name,
                op: TextOp::Contains,
                value: sub,
                case_sensitive: false,
            }, &facts, &clock);
            prop_assert!(contains);

            let between = evaluate_condition(&Condition::Size {
                op: SizeOp::Between,
                value: a,
                max_value: Some(b),
                unit: SizeUnit::Kilobytes,
            }, &facts, &clock);
            let lo = a.min(b) * 1_000;
            let hi = a.max(b) * 1_000;
            prop_assert_eq!(between, file_size >= lo && file_size <= hi);
        }

        #[test]
        fn r_grp_05_proptest_boolean_algebra_laws(
            a_match in any::<bool>(),
            b_match in any::<bool>(),
            c_match in any::<bool>(),
        ) {
            let clock = FixedClock::utc(1_772_942_400_000);
            let facts = sample_facts();
            let make_leaf = |m: bool| ConditionNode::Leaf {
                condition: Condition::Hidden { is_hidden: !m }, // facts.is_hidden == false, so !m == false -> m
            };
            let a = make_leaf(a_match);
            let b = make_leaf(b_match);
            let c = make_leaf(c_match);

            prop_assert_eq!(evaluate_node(&a, &facts, &clock).matched, a_match);

            // Double negation: Not(Not(a)) == a
            let nn_a = ConditionNode::Not {
                child: Box::new(ConditionNode::Not {
                    child: Box::new(a.clone()),
                }),
            };
            prop_assert_eq!(
                evaluate_node(&nn_a, &facts, &clock).matched,
                evaluate_node(&a, &facts, &clock).matched
            );

            // De Morgan 1: Not(All([a, b, c])) == Any([Not(a), Not(b), Not(c)])
            let lhs1 = ConditionNode::Not {
                child: Box::new(ConditionNode::All {
                    children: vec![a.clone(), b.clone(), c.clone()],
                }),
            };
            let rhs1 = ConditionNode::Any {
                children: vec![
                    ConditionNode::Not { child: Box::new(a.clone()) },
                    ConditionNode::Not { child: Box::new(b.clone()) },
                    ConditionNode::Not { child: Box::new(c.clone()) },
                ],
            };
            prop_assert_eq!(
                evaluate_node(&lhs1, &facts, &clock).matched,
                evaluate_node(&rhs1, &facts, &clock).matched
            );

            // De Morgan 2: Not(Any([a, b])) == All([Not(a), Not(b)])
            let lhs2 = ConditionNode::Not {
                child: Box::new(ConditionNode::Any {
                    children: vec![a.clone(), b.clone()],
                }),
            };
            let rhs2 = ConditionNode::All {
                children: vec![
                    ConditionNode::Not { child: Box::new(a) },
                    ConditionNode::Not { child: Box::new(b) },
                ],
            };
            prop_assert_eq!(
                evaluate_node(&lhs2, &facts, &clock).matched,
                evaluate_node(&rhs2, &facts, &clock).matched
            );
        }
    }
}
