//! Pure rule evaluation pipeline (P3-T06; FR-050–054).
//!
//! Evaluates a prioritized list of rules against a `FileFacts` snapshot using an
//! injectable `Clock`. Performs zero filesystem or database I/O.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;
use unicode_normalization::UnicodeNormalization;

use crate::engine::paths::MAX_COMPONENT_BYTES;
use crate::rules::conditions::evaluate_node;
use crate::rules::models::{Action, DatePlacement, DateSource, ExecutionMode, FileFacts, Rule};
use crate::rules::templates::{
    render_filename_template, render_subpath_template, sanitize_variable_value,
};
use crate::rules::validate::FORBIDDEN_EXECUTABLE_EXTENSIONS;
use crate::utils::clock::Clock;

pub const MAX_MOVES_PER_PASS: usize = 1;
pub const MAX_ACTIONS_PER_PASS: usize = 10;
pub const MAX_RULE_CHAIN_DEPTH: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlannedOp {
    Move { destination_dir: String },
    Copy { destination_dir: String },
    Rename { new_name: String },
    CreateFolder { path: String },
    Notify { message: String },
    MarkForReview { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedAction {
    pub rule_id: String,
    pub rule_name: String,
    pub op: PlannedOp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MatchedRuleTrace {
    pub rule_id: String,
    pub rule_name: String,
    pub matched_leaves: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum EvalWarning {
    RuleCycleDetected {
        from_rule_id: String,
        target_rule_id: String,
    },
    MaxChainDepthExceeded {
        from_rule_id: String,
        target_rule_id: String,
        max_depth: u32,
    },
    MoveLimitReached {
        rule_id: String,
    },
    ActionLimitReached {
        rule_id: String,
    },
    DestinationLoopGuarded {
        rule_id: String,
        destination: String,
    },
    TemplateFailed {
        rule_id: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationOutcome {
    pub matched_rules: Vec<MatchedRuleTrace>,
    pub actions: Vec<EvaluatedAction>,
    pub final_directory: String,
    pub final_filename: String,
    pub final_path: String,
    pub skipped: bool,
    pub warnings: Vec<EvalWarning>,
}

struct EvalState {
    original_facts: FileFacts,
    working_facts: FileFacts,
    current_dir: String,
    matched_rules: Vec<MatchedRuleTrace>,
    actions: Vec<EvaluatedAction>,
    warnings: Vec<EvalWarning>,
    moves_count: usize,
    skipped: bool,
    stop_processing: bool,
}

pub fn evaluate_rules(rules: &[Rule], file: &FileFacts, clock: &dyn Clock) -> EvaluationOutcome {
    let parent = Path::new(&file.path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.source_folder_path.clone());

    let mut rule_by_id: HashMap<&str, &Rule> = HashMap::with_capacity(rules.len());
    for r in rules {
        rule_by_id.insert(r.id.as_str(), r);
    }

    let mut top_rules: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.enabled && rule_applies_to_folder(r, file))
        .collect();
    top_rules.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut state = EvalState {
        original_facts: file.clone(),
        working_facts: file.clone(),
        current_dir: parent,
        matched_rules: Vec::new(),
        actions: Vec::new(),
        warnings: Vec::new(),
        moves_count: 0,
        skipped: false,
        stop_processing: false,
    };

    let mut use_working_facts_for_next = false;

    for rule in top_rules {
        if state.skipped || state.stop_processing {
            break;
        }

        let target_facts = if use_working_facts_for_next {
            &state.working_facts
        } else {
            &state.original_facts
        };

        let group_res = evaluate_node(&rule.conditions, target_facts, clock);
        if !group_res.matched {
            continue;
        }

        state.matched_rules.push(MatchedRuleTrace {
            rule_id: rule.id.clone(),
            rule_name: rule.name.clone(),
            matched_leaves: group_res.matched_leaves,
        });

        let actions_before = state.actions.len();
        let moves_before = state.moves_count;
        let mut visited = HashSet::new();
        visited.insert(rule.id.clone());

        apply_rule_actions(rule, &rule_by_id, clock, &mut state, &mut visited, 0);

        if state.skipped || state.stop_processing {
            break;
        }

        match rule.execution_mode {
            ExecutionMode::FirstMatch => break,
            ExecutionMode::Continue => {
                use_working_facts_for_next = false;
            }
            ExecutionMode::Sequential => {
                use_working_facts_for_next = true;
            }
            ExecutionMode::StopAfterMove => {
                if state.moves_count > moves_before {
                    break;
                }
            }
            ExecutionMode::StopAfterAction => {
                if state.actions.len() > actions_before {
                    break;
                }
            }
        }
    }

    let final_filename = state.working_facts.name.clone();
    let final_directory = state.current_dir.clone();
    let final_path = if final_directory.is_empty() {
        final_filename.clone()
    } else {
        format!(
            "{}/{}",
            final_directory.trim_end_matches('/'),
            final_filename
        )
    };

    EvaluationOutcome {
        matched_rules: state.matched_rules,
        actions: state.actions,
        final_directory,
        final_filename,
        final_path,
        skipped: state.skipped,
        warnings: state.warnings,
    }
}

fn rule_applies_to_folder(rule: &Rule, file: &FileFacts) -> bool {
    if rule.source_folders.is_empty() {
        return true;
    }
    let src_path_nfc: String = file.source_folder_path.nfc().collect();
    rule.source_folders.iter().any(|sf| {
        let sf_nfc: String = sf.trim().nfc().collect();
        sf_nfc == file.source_folder_id || sf_nfc == src_path_nfc
    })
}

fn apply_rule_actions(
    rule: &Rule,
    rule_by_id: &HashMap<&str, &Rule>,
    clock: &dyn Clock,
    state: &mut EvalState,
    visited: &mut HashSet<String>,
    depth: usize,
) {
    for action in &rule.actions {
        if state.skipped || state.stop_processing {
            return;
        }

        if let Action::ApplyAnotherRule { rule_id } = action {
            let target_id = rule_id.trim();
            if visited.contains(target_id) {
                state.warnings.push(EvalWarning::RuleCycleDetected {
                    from_rule_id: rule.id.clone(),
                    target_rule_id: target_id.to_owned(),
                });
                continue;
            }
            if depth + 1 >= MAX_RULE_CHAIN_DEPTH {
                state.warnings.push(EvalWarning::MaxChainDepthExceeded {
                    from_rule_id: rule.id.clone(),
                    target_rule_id: target_id.to_owned(),
                    max_depth: MAX_RULE_CHAIN_DEPTH as u32,
                });
                continue;
            }
            if let Some(target_rule) = rule_by_id.get(target_id) {
                let sub_res = evaluate_node(&target_rule.conditions, &state.working_facts, clock);
                if sub_res.matched {
                    state.matched_rules.push(MatchedRuleTrace {
                        rule_id: target_rule.id.clone(),
                        rule_name: target_rule.name.clone(),
                        matched_leaves: sub_res.matched_leaves,
                    });
                    visited.insert(target_id.to_owned());
                    apply_rule_actions(target_rule, rule_by_id, clock, state, visited, depth + 1);
                    visited.remove(target_id);
                }
            }
            continue;
        }

        if matches!(action, Action::Skip) {
            state.skipped = true;
            return;
        }
        if matches!(action, Action::StopProcessing) {
            state.stop_processing = true;
            return;
        }

        if state.actions.len() >= MAX_ACTIONS_PER_PASS {
            state.warnings.push(EvalWarning::ActionLimitReached {
                rule_id: rule.id.clone(),
            });
            return;
        }

        execute_single_action(action, rule, clock, state);
    }
}

fn execute_single_action(action: &Action, rule: &Rule, clock: &dyn Clock, state: &mut EvalState) {
    let date_source = rule.date_source;
    match action {
        Action::Move { destination } => {
            if state.moves_count >= MAX_MOVES_PER_PASS {
                state.warnings.push(EvalWarning::MoveLimitReached {
                    rule_id: rule.id.clone(),
                });
                return;
            }
            match resolve_dir_string(None, destination, &state.working_facts, date_source, clock) {
                Ok(dest_dir) => {
                    if is_same_dir(&dest_dir, &state.current_dir) {
                        state.warnings.push(EvalWarning::DestinationLoopGuarded {
                            rule_id: rule.id.clone(),
                            destination: dest_dir,
                        });
                        return;
                    }
                    state.current_dir = dest_dir.clone();
                    state.working_facts.path = format!(
                        "{}/{}",
                        dest_dir.trim_end_matches('/'),
                        state.working_facts.name
                    );
                    state.moves_count += 1;
                    state.actions.push(EvaluatedAction {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        op: PlannedOp::Move {
                            destination_dir: dest_dir,
                        },
                    });
                }
                Err(msg) => state.warnings.push(EvalWarning::TemplateFailed {
                    rule_id: rule.id.clone(),
                    message: msg,
                }),
            }
        }
        Action::MoveIntoDynamicFolder {
            root,
            subpath_template,
        } => {
            if state.moves_count >= MAX_MOVES_PER_PASS {
                state.warnings.push(EvalWarning::MoveLimitReached {
                    rule_id: rule.id.clone(),
                });
                return;
            }
            match resolve_dir_string(
                root.as_deref(),
                subpath_template,
                &state.working_facts,
                date_source,
                clock,
            ) {
                Ok(dest_dir) => {
                    if is_same_dir(&dest_dir, &state.current_dir) {
                        state.warnings.push(EvalWarning::DestinationLoopGuarded {
                            rule_id: rule.id.clone(),
                            destination: dest_dir,
                        });
                        return;
                    }
                    state.current_dir = dest_dir.clone();
                    state.working_facts.path = format!(
                        "{}/{}",
                        dest_dir.trim_end_matches('/'),
                        state.working_facts.name
                    );
                    state.moves_count += 1;
                    state.actions.push(EvaluatedAction {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        op: PlannedOp::Move {
                            destination_dir: dest_dir,
                        },
                    });
                }
                Err(msg) => state.warnings.push(EvalWarning::TemplateFailed {
                    rule_id: rule.id.clone(),
                    message: msg,
                }),
            }
        }
        Action::Copy { destination } => {
            match resolve_dir_string(None, destination, &state.working_facts, date_source, clock) {
                Ok(dest_dir) => {
                    state.actions.push(EvaluatedAction {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        op: PlannedOp::Copy {
                            destination_dir: dest_dir,
                        },
                    });
                }
                Err(msg) => state.warnings.push(EvalWarning::TemplateFailed {
                    rule_id: rule.id.clone(),
                    message: msg,
                }),
            }
        }
        Action::CreateDestinationFolder { path } => {
            match resolve_dir_string(None, path, &state.working_facts, date_source, clock) {
                Ok(dir_path) => {
                    state.actions.push(EvaluatedAction {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        op: PlannedOp::CreateFolder { path: dir_path },
                    });
                }
                Err(msg) => state.warnings.push(EvalWarning::TemplateFailed {
                    rule_id: rule.id.clone(),
                    message: msg,
                }),
            }
        }
        Action::Rename { template } => {
            match render_filename_template(template, &state.working_facts, date_source, clock) {
                Ok(new_name) => apply_new_filename(rule, state, new_name),
                Err(e) => state.warnings.push(EvalWarning::TemplateFailed {
                    rule_id: rule.id.clone(),
                    message: e.to_string(),
                }),
            }
        }
        Action::AddPrefix { prefix } => {
            let new_stem = format!("{prefix}{}", state.working_facts.stem);
            let sanitized_stem = sanitize_variable_value(&new_stem, &state.working_facts.stem);
            let new_name = rebuild_name(&sanitized_stem, &state.working_facts.extension);
            if new_name.len() <= MAX_COMPONENT_BYTES {
                apply_new_filename(rule, state, new_name);
            }
        }
        Action::AddSuffix { suffix } => {
            let new_stem = format!("{}{suffix}", state.working_facts.stem);
            let sanitized_stem = sanitize_variable_value(&new_stem, &state.working_facts.stem);
            let new_name = rebuild_name(&sanitized_stem, &state.working_facts.extension);
            if new_name.len() <= MAX_COMPONENT_BYTES {
                apply_new_filename(rule, state, new_name);
            }
        }
        Action::RemoveText {
            text,
            case_sensitive,
        } => {
            let new_stem = replace_in_str(&state.working_facts.stem, text, "", *case_sensitive);
            let sanitized_stem = sanitize_variable_value(&new_stem, &state.working_facts.stem);
            let new_name = rebuild_name(&sanitized_stem, &state.working_facts.extension);
            if new_name.len() <= MAX_COMPONENT_BYTES {
                apply_new_filename(rule, state, new_name);
            }
        }
        Action::ReplaceText {
            find,
            replace,
            case_sensitive,
        } => {
            let new_stem =
                replace_in_str(&state.working_facts.stem, find, replace, *case_sensitive);
            let sanitized_stem = sanitize_variable_value(&new_stem, &state.working_facts.stem);
            let new_name = rebuild_name(&sanitized_stem, &state.working_facts.extension);
            if new_name.len() <= MAX_COMPONENT_BYTES {
                apply_new_filename(rule, state, new_name);
            }
        }
        Action::AddDateToFilename { placement, format } => {
            let fmt = if format.trim().is_empty() {
                "{date}"
            } else {
                format.trim()
            };
            let tpl = match placement {
                DatePlacement::Prefix => format!("{fmt}_{{original_name}}"),
                DatePlacement::Suffix => format!("{{original_name}}_{fmt}"),
            };
            match render_filename_template(&tpl, &state.working_facts, date_source, clock) {
                Ok(new_name) => apply_new_filename(rule, state, new_name),
                Err(e) => state.warnings.push(EvalWarning::TemplateFailed {
                    rule_id: rule.id.clone(),
                    message: e.to_string(),
                }),
            }
        }
        Action::ChangeExtension {
            new_extension,
            confirm_unsafe,
        } => {
            let ext = new_extension.trim().trim_start_matches('.').to_lowercase();
            if !*confirm_unsafe
                || ext.is_empty()
                || FORBIDDEN_EXECUTABLE_EXTENSIONS.contains(&ext.as_str())
            {
                return;
            }
            let new_name = rebuild_name(&state.working_facts.stem, &ext);
            if new_name.len() <= MAX_COMPONENT_BYTES {
                apply_new_filename(rule, state, new_name);
            }
        }
        Action::Notify { message } => {
            state.actions.push(EvaluatedAction {
                rule_id: rule.id.clone(),
                rule_name: rule.name.clone(),
                op: PlannedOp::Notify {
                    message: message.clone(),
                },
            });
        }
        Action::MarkForReview { reason } => {
            state.actions.push(EvaluatedAction {
                rule_id: rule.id.clone(),
                rule_name: rule.name.clone(),
                op: PlannedOp::MarkForReview {
                    reason: reason.clone(),
                },
            });
        }
        Action::Skip | Action::StopProcessing | Action::ApplyAnotherRule { .. } => {}
    }
}

fn apply_new_filename(rule: &Rule, state: &mut EvalState, new_name: String) {
    if new_name == state.working_facts.name {
        return;
    }
    let (stem, ext) = FileFacts::split_name(&new_name);
    state.working_facts.name = new_name.clone();
    state.working_facts.stem = stem;
    state.working_facts.extension = ext;
    state.working_facts.path = format!(
        "{}/{}",
        state.current_dir.trim_end_matches('/'),
        state.working_facts.name
    );
    state.actions.push(EvaluatedAction {
        rule_id: rule.id.clone(),
        rule_name: rule.name.clone(),
        op: PlannedOp::Rename { new_name },
    });
}

fn rebuild_name(stem: &str, ext: &str) -> String {
    let e = ext.trim().trim_start_matches('.');
    if e.is_empty() {
        stem.to_owned()
    } else {
        format!("{stem}.{e}")
    }
}

fn replace_in_str(haystack: &str, needle: &str, replacement: &str, case_sensitive: bool) -> String {
    if needle.is_empty() {
        return haystack.to_owned();
    }
    if case_sensitive {
        return haystack.replace(needle, replacement);
    }
    let escaped = regex::escape(needle);
    let Ok(re) = regex::RegexBuilder::new(&escaped)
        .case_insensitive(true)
        .build()
    else {
        return haystack.to_owned();
    };
    re.replace_all(haystack, regex::NoExpand(replacement))
        .into_owned()
}

fn resolve_dir_string(
    explicit_root: Option<&str>,
    template: &str,
    facts: &FileFacts,
    date_source: DateSource,
    clock: &dyn Clock,
) -> Result<String, String> {
    let trimmed = template.trim();
    if let Some(rel) = trimmed.strip_prefix('/') {
        let rendered =
            render_subpath_template(rel, facts, date_source, clock).map_err(|e| e.to_string())?;
        return Ok(format!("/{rendered}"));
    }

    let rendered =
        render_subpath_template(trimmed, facts, date_source, clock).map_err(|e| e.to_string())?;
    let base = explicit_root
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(facts.source_folder_path.as_str())
        .trim_end_matches('/');

    if base.is_empty() {
        Ok(rendered)
    } else {
        Ok(format!("{base}/{rendered}"))
    }
}

fn is_same_dir(a: &str, b: &str) -> bool {
    let a_nfc: String = a.trim_end_matches('/').nfc().collect();
    let b_nfc: String = b.trim_end_matches('/').nfc().collect();
    a_nfc == b_nfc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::models::{
        Action, Condition, ConditionNode, DatePlacement, DateSource, ExecutionMode, RuleOrigin,
        SizeOp, SizeUnit, TextFieldKind, TextOp,
    };
    use crate::utils::clock::FixedClock;
    use chrono::TimeZone;

    fn utc_ms(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> i64 {
        chrono::Utc
            .with_ymd_and_hms(y, m, d, hh, mm, ss)
            .single()
            .expect("valid utc datetime")
            .timestamp_millis()
    }

    fn base_file() -> FileFacts {
        FileFacts {
            path: "/Users/alice/Downloads/Invoice_Acme_2026.pdf".into(),
            name: "Invoice_Acme_2026.pdf".into(),
            stem: "Invoice_Acme_2026".into(),
            extension: "pdf".into(),
            mime: Some("application/pdf".into()),
            category: "Documents".into(),
            size: 150_000,
            created_ms: Some(utc_ms(2026, 11, 14, 9, 0, 0)),
            modified_ms: utc_ms(2026, 11, 14, 9, 0, 0),
            added_ms: Some(utc_ms(2026, 11, 14, 9, 0, 0)),
            is_hidden: false,
            is_duplicate: false,
            content_hash: None,
            source_folder_id: "f-dl".into(),
            source_folder_path: "/Users/alice/Downloads".into(),
            source_folder_name: "Downloads".into(),
        }
    }

    fn make_rule(
        id: &str,
        name: &str,
        priority: i32,
        mode: ExecutionMode,
        conditions: ConditionNode,
        actions: Vec<Action>,
    ) -> Rule {
        Rule {
            id: id.into(),
            name: name.into(),
            description: None,
            enabled: true,
            priority,
            source_folders: Vec::new(),
            conditions,
            actions,
            date_source: DateSource::Modified,
            execution_mode: mode,
            origin: RuleOrigin::User,
            last_run_at: None,
            stat_matched: 0,
            stat_succeeded: 0,
            stat_failed: 0,
            created_at: 1000,
            updated_at: 1000,
        }
    }

    #[test]
    fn r_eval_01_priority_order_enabled_and_source_folder_scoping() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let file = base_file();

        let match_pdf = ConditionNode::Leaf {
            condition: Condition::ExtensionGroup {
                extensions: vec!["pdf".into()],
            },
        };

        let mut disabled_high = make_rule(
            "r-disabled",
            "Disabled High",
            1,
            ExecutionMode::FirstMatch,
            match_pdf.clone(),
            vec![Action::Move {
                destination: "ShouldNotRun".into(),
            }],
        );
        disabled_high.enabled = false;

        let mut other_folder = make_rule(
            "r-other-folder",
            "Other Folder",
            2,
            ExecutionMode::FirstMatch,
            match_pdf.clone(),
            vec![Action::Move {
                destination: "OtherFolderDest".into(),
            }],
        );
        other_folder.source_folders = vec!["f-desktop".into()];

        let low_prio = make_rule(
            "r-low",
            "Low Priority",
            20,
            ExecutionMode::FirstMatch,
            match_pdf.clone(),
            vec![Action::Move {
                destination: "GenericPDFs".into(),
            }],
        );

        let high_prio = make_rule(
            "r-high",
            "High Priority",
            10,
            ExecutionMode::FirstMatch,
            match_pdf,
            vec![Action::Move {
                destination: "Invoices".into(),
            }],
        );

        // Pass out of order: evaluator must sort by priority and pick r-high (priority 10)
        let out = evaluate_rules(
            &[low_prio, disabled_high, other_folder, high_prio],
            &file,
            &clock,
        );
        assert_eq!(out.matched_rules.len(), 1);
        assert_eq!(out.matched_rules[0].rule_id, "r-high");
        assert_eq!(out.final_directory, "/Users/alice/Downloads/Invoices");
    }

    #[test]
    fn r_eval_02_execution_modes_continue_stop_after_move_and_sequential() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let file = base_file();

        let match_pdf = ConditionNode::Leaf {
            condition: Condition::ExtensionGroup {
                extensions: vec!["pdf".into()],
            },
        };

        // Sequential mode: Rule 1 adds prefix "PAID_", Rule 2 matches Name StartsWith "PAID_" and moves!
        let r1 = make_rule(
            "r1",
            "Tag Paid",
            1,
            ExecutionMode::Sequential,
            match_pdf,
            vec![Action::AddPrefix {
                prefix: "PAID_".into(),
            }],
        );
        let r2 = make_rule(
            "r2",
            "Move Paid",
            2,
            ExecutionMode::StopAfterMove,
            ConditionNode::Leaf {
                condition: Condition::Text {
                    field: TextFieldKind::Name,
                    op: TextOp::StartsWith,
                    value: "PAID_".into(),
                    case_sensitive: true,
                },
            },
            vec![Action::Move {
                destination: "PaidInvoices".into(),
            }],
        );
        let r3 = make_rule(
            "r3",
            "After Move Should Not Run",
            3,
            ExecutionMode::FirstMatch,
            ConditionNode::All { children: vec![] },
            vec![Action::Notify {
                message: "Should not run after StopAfterMove".into(),
            }],
        );

        let out = evaluate_rules(&[r1, r2, r3], &file, &clock);
        assert_eq!(out.matched_rules.len(), 2);
        assert_eq!(out.final_filename, "PAID_Invoice_Acme_2026.pdf");
        assert_eq!(out.final_directory, "/Users/alice/Downloads/PaidInvoices");
        assert_eq!(
            out.final_path,
            "/Users/alice/Downloads/PaidInvoices/PAID_Invoice_Acme_2026.pdf"
        );
    }

    #[test]
    fn r_eval_03_apply_another_rule_cycle_detection_and_depth_limit() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let file = base_file();
        let always = ConditionNode::All { children: vec![] };

        // Indirect cycle: A -> B -> A
        let r_a = make_rule(
            "rule-a",
            "Rule A",
            1,
            ExecutionMode::FirstMatch,
            always.clone(),
            vec![
                Action::AddPrefix {
                    prefix: "A_".into(),
                },
                Action::ApplyAnotherRule {
                    rule_id: "rule-b".into(),
                },
            ],
        );
        let mut r_b = make_rule(
            "rule-b",
            "Rule B",
            2,
            ExecutionMode::FirstMatch,
            always.clone(),
            vec![
                Action::AddSuffix {
                    suffix: "_B".into(),
                },
                Action::ApplyAnotherRule {
                    rule_id: "rule-a".into(),
                },
            ],
        );
        r_b.enabled = false; // Even as a helper rule invoked via ApplyAnotherRule, cycle is caught

        let out = evaluate_rules(&[r_a, r_b], &file, &clock);
        assert_eq!(out.final_filename, "A_Invoice_Acme_2026_B.pdf");
        assert!(out.warnings.iter().any(|w| matches!(
            w,
            EvalWarning::RuleCycleDetected {
                from_rule_id,
                target_rule_id
            } if from_rule_id == "rule-b" && target_rule_id == "rule-a"
        )));

        // Depth limit > MAX_RULE_CHAIN_DEPTH (5)
        let mut chain = Vec::new();
        for i in 0..=7 {
            let mut r = make_rule(
                &format!("c{i}"),
                &format!("Chain {i}"),
                i + 1,
                ExecutionMode::FirstMatch,
                always.clone(),
                vec![Action::ApplyAnotherRule {
                    rule_id: format!("c{}", i + 1),
                }],
            );
            if i > 0 {
                r.enabled = false;
            }
            chain.push(r);
        }
        let out_depth = evaluate_rules(&chain, &file, &clock);
        assert!(out_depth
            .warnings
            .iter()
            .any(|w| matches!(w, EvalWarning::MaxChainDepthExceeded { .. })));
    }

    #[test]
    fn r_eval_04_per_pass_move_limit_and_action_limit() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let file = base_file();
        let always = ConditionNode::All { children: vec![] };

        let r1 = make_rule(
            "r1",
            "Move First",
            1,
            ExecutionMode::Continue,
            always.clone(),
            vec![Action::Move {
                destination: "FirstDest".into(),
            }],
        );
        let r2 = make_rule(
            "r2",
            "Move Second",
            2,
            ExecutionMode::Continue,
            always,
            vec![Action::Move {
                destination: "SecondDest".into(),
            }],
        );

        let out = evaluate_rules(&[r1, r2], &file, &clock);
        // Only the first move is allowed; the second is blocked with MoveLimitReached
        assert_eq!(out.final_directory, "/Users/alice/Downloads/FirstDest");
        assert!(out
            .warnings
            .iter()
            .any(|w| matches!(w, EvalWarning::MoveLimitReached { rule_id } if rule_id == "r2")));
    }

    #[test]
    fn r_eval_05_runtime_destination_loop_guard_prevents_re_moving_already_organized_file() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let mut file = base_file();
        // Suppose the file was ALREADY moved into /Users/alice/Downloads/Invoices/2026
        // inside a recursive monitored folder /Users/alice/Downloads:
        file.path = "/Users/alice/Downloads/Invoices/2026/Invoice_Acme_2026.pdf".into();

        let rule = make_rule(
            "r-inv",
            "Invoices",
            1,
            ExecutionMode::FirstMatch,
            ConditionNode::Leaf {
                condition: Condition::ExtensionGroup {
                    extensions: vec!["pdf".into()],
                },
            },
            vec![Action::MoveIntoDynamicFolder {
                root: None,
                subpath_template: "Invoices/{year}".into(),
            }],
        );

        let out = evaluate_rules(&[rule], &file, &clock);
        assert!(
            out.actions.is_empty(),
            "must not plan a move onto the current directory: {:?}",
            out.actions
        );
        assert!(out
            .warnings
            .iter()
            .any(|w| matches!(w, EvalWarning::DestinationLoopGuarded { .. })));
    }

    #[test]
    fn r_eval_06_spec_6_2_nested_invoice_rule_evaluates_end_to_end() {
        let clock = FixedClock::utc(utc_ms(2026, 11, 14, 12, 0, 0));
        let file = base_file();

        // Spec §6.2 style rule:
        // ALL [
        //   Extension in ["pdf"],
        //   Size > 1 KB,
        //   ANY [ Name contains "invoice", Name contains "receipt", Name contains "bill" ],
        //   NOT ( Name contains "draft" )
        // ]
        // Actions:
        //   1. ReplaceText "_" -> "-"
        //   2. AddDateToFilename (Prefix, "{date}")
        //   3. MoveIntoDynamicFolder "Finance/Invoices/{year}/{month_name}"
        let rule = make_rule(
            "spec-6-2",
            "Finance Invoices",
            1,
            ExecutionMode::FirstMatch,
            ConditionNode::All {
                children: vec![
                    ConditionNode::Leaf {
                        condition: Condition::ExtensionGroup {
                            extensions: vec!["pdf".into()],
                        },
                    },
                    ConditionNode::Leaf {
                        condition: Condition::Size {
                            op: SizeOp::GreaterThan,
                            value: 1,
                            max_value: None,
                            unit: SizeUnit::Kilobytes,
                        },
                    },
                    ConditionNode::Any {
                        children: vec![
                            ConditionNode::Leaf {
                                condition: Condition::Text {
                                    field: TextFieldKind::Name,
                                    op: TextOp::Contains,
                                    value: "invoice".into(),
                                    case_sensitive: false,
                                },
                            },
                            ConditionNode::Leaf {
                                condition: Condition::Text {
                                    field: TextFieldKind::Name,
                                    op: TextOp::Contains,
                                    value: "receipt".into(),
                                    case_sensitive: false,
                                },
                            },
                        ],
                    },
                    ConditionNode::Not {
                        child: Box::new(ConditionNode::Leaf {
                            condition: Condition::Text {
                                field: TextFieldKind::Name,
                                op: TextOp::Contains,
                                value: "draft".into(),
                                case_sensitive: false,
                            },
                        }),
                    },
                ],
            },
            vec![
                Action::ReplaceText {
                    find: "_".into(),
                    replace: "-".into(),
                    case_sensitive: false,
                },
                Action::AddDateToFilename {
                    placement: DatePlacement::Prefix,
                    format: "{date}".into(),
                },
                Action::MoveIntoDynamicFolder {
                    root: None,
                    subpath_template: "Finance/Invoices/{year}/{month_name}".into(),
                },
            ],
        );

        let out = evaluate_rules(&[rule], &file, &clock);
        assert_eq!(out.matched_rules.len(), 1);
        assert_eq!(out.final_filename, "2026-11-14_Invoice-Acme-2026.pdf");
        assert_eq!(
            out.final_directory,
            "/Users/alice/Downloads/Finance/Invoices/2026/November"
        );
        assert_eq!(
            out.final_path,
            "/Users/alice/Downloads/Finance/Invoices/2026/November/2026-11-14_Invoice-Acme-2026.pdf"
        );
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn bug_015_add_prefix_and_suffix_allow_literal_braces_without_template_error() {
        let clock = FixedClock::utc(1_772_942_400_000);
        let file = base_file();
        let rule = make_rule(
            "r-braces",
            "Braces Rule",
            1,
            ExecutionMode::FirstMatch,
            ConditionNode::All { children: vec![] },
            vec![
                Action::AddPrefix {
                    prefix: "{Draft}_".into(),
                },
                Action::AddSuffix {
                    suffix: "_{v1}".into(),
                },
            ],
        );
        let out = evaluate_rules(&[rule], &file, &clock);
        assert!(
            out.warnings.is_empty(),
            "unexpected warnings: {:?}",
            out.warnings
        );
        assert_eq!(out.final_filename, "{Draft}_Invoice_Acme_2026_{v1}.pdf");
    }
}
