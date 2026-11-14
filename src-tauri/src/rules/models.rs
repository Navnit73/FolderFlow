//! Rule domain models and versioned JSON envelopes (P3-T01, FR-040–055, FR-071).

use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, AppResult};

pub const RULE_SCHEMA_VERSION: u32 = 1;
pub const MAX_CONDITION_DEPTH: usize = 5;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    FirstMatch,
    Continue,
    StopAfterMove,
    StopAfterAction,
    Sequential,
}

impl ExecutionMode {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::FirstMatch => "first_match",
            Self::Continue => "continue",
            Self::StopAfterMove => "stop_after_move",
            Self::StopAfterAction => "stop_after_action",
            Self::Sequential => "sequential",
        }
    }

    pub fn from_db(s: &str) -> AppResult<Self> {
        match s {
            "first_match" => Ok(Self::FirstMatch),
            "continue" => Ok(Self::Continue),
            "stop_after_move" => Ok(Self::StopAfterMove),
            "stop_after_action" => Ok(Self::StopAfterAction),
            "sequential" => Ok(Self::Sequential),
            other => Err(AppError::validation(
                "executionMode",
                format!("unknown execution mode: {other}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleOrigin {
    #[default]
    User,
    Preset,
    Import,
}

impl RuleOrigin {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Preset => "preset",
            Self::Import => "import",
        }
    }

    pub fn from_db(s: &str) -> AppResult<Self> {
        match s {
            "user" => Ok(Self::User),
            "preset" => Ok(Self::Preset),
            "import" => Ok(Self::Import),
            other => Err(AppError::validation(
                "origin",
                format!("unknown rule origin: {other}"),
            )),
        }
    }
}

/// Which timestamp on the file populates `{year}`, `{month}`, `{day}`, `{date}`, `{month_name}` (FR-071).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DateSource {
    #[default]
    Modified,
    Created,
    Added,
    Now,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TextFieldKind {
    Name,
    Stem,
    Extension,
    MimeType,
    FullPath,
    ParentFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TextOp {
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Equals,
    NotEquals,
    MatchesRegex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SizeOp {
    GreaterThan,
    LessThan,
    Equals,
    Between,
}

/// Decimal storage units (FR-044: 1 KB = 1000 B).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SizeUnit {
    Bytes,
    #[default]
    Kilobytes,
    Megabytes,
    Gigabytes,
}

impl SizeUnit {
    pub const fn multiplier(self) -> u64 {
        match self {
            Self::Bytes => 1,
            Self::Kilobytes => 1_000,
            Self::Megabytes => 1_000_000,
            Self::Gigabytes => 1_000_000_000,
        }
    }

    pub fn to_bytes(self, n: u64) -> u64 {
        n.saturating_mul(self.multiplier())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DateField {
    Created,
    #[default]
    Modified,
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DateConditionOp {
    Before { date: String },
    After { date: String },
    WithinRange { start: String, end: String },
    OlderThanDays { days: u32 },
    NewerThanDays { days: u32 },
    Today,
    Yesterday,
    ThisWeek,
    ThisMonth,
    ThisYear,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum LocationOp {
    SourceFolderEquals {
        folder: String,
    },
    SourceFolderContains {
        text: String,
    },
    InsideDirectory {
        path: String,
    },
    InsideSubdirectory {
        #[serde(default)]
        subpath: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    Text {
        field: TextFieldKind,
        op: TextOp,
        value: String,
        #[serde(default)]
        case_sensitive: bool,
    },
    Category {
        category_id: String,
        #[serde(default)]
        negate: bool,
    },
    Size {
        op: SizeOp,
        #[specta(type = u32)]
        value: u64,
        #[serde(default)]
        #[specta(type = Option<u32>)]
        max_value: Option<u64>,
        #[serde(default)]
        unit: SizeUnit,
    },
    Date {
        field: DateField,
        op: DateConditionOp,
    },
    Location {
        op: LocationOp,
    },
    Hidden {
        is_hidden: bool,
    },
    EmptyFile {
        is_empty: bool,
    },
    DuplicateStatus {
        is_duplicate: bool,
    },
    ContentHash {
        hash: String,
    },
    FilenameGlob {
        pattern: String,
        #[serde(default)]
        case_sensitive: bool,
    },
    ExtensionGroup {
        extensions: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConditionNode {
    All { children: Vec<ConditionNode> },
    Any { children: Vec<ConditionNode> },
    Not { child: Box<ConditionNode> },
    Leaf { condition: Condition },
}

impl ConditionNode {
    /// Inspects a condition tree and reports `(needs_content_hash, needs_duplicate_status)`.
    pub fn hash_requirements(&self) -> (bool, bool) {
        match self {
            ConditionNode::Leaf { condition } => match condition {
                Condition::ContentHash { .. } => (true, false),
                Condition::DuplicateStatus { .. } => (false, true),
                _ => (false, false),
            },
            ConditionNode::Not { child } => child.hash_requirements(),
            ConditionNode::All { children } | ConditionNode::Any { children } => {
                children.iter().fold((false, false), |acc, c| {
                    let (h, d) = c.hash_requirements();
                    (acc.0 || h, acc.1 || d)
                })
            }
        }
    }
}

/// Inspects enabled rules and returns `(needs_content_hash, needs_duplicate_status)`.
pub fn rules_hash_requirements(rules: &[Rule]) -> (bool, bool) {
    rules
        .iter()
        .filter(|r| r.enabled)
        .fold((false, false), |acc, r| {
            let (h, d) = r.conditions.hash_requirements();
            (acc.0 || h, acc.1 || d)
        })
}

/// Filters rules to `rule_id` (forced enabled for manual preview/single-rule runs) when set.
pub fn scope_rules(rules: &[Rule], rule_id: Option<&str>) -> Vec<Rule> {
    if let Some(id) = rule_id {
        rules
            .iter()
            .filter(|r| r.id == id)
            .map(|r| {
                let mut cloned = r.clone();
                cloned.enabled = true;
                cloned
            })
            .collect()
    } else {
        rules.to_vec()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DatePlacement {
    #[default]
    Prefix,
    Suffix,
}

fn default_date_format() -> String {
    "{date}".to_owned()
}

/// Rule actions (FR-050). No shell/script/command execution exists (FR-051).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Move {
        destination: String,
    },
    MoveIntoDynamicFolder {
        #[serde(default)]
        root: Option<String>,
        subpath_template: String,
    },
    Copy {
        destination: String,
    },
    Rename {
        template: String,
    },
    AddPrefix {
        prefix: String,
    },
    AddSuffix {
        suffix: String,
    },
    RemoveText {
        text: String,
        #[serde(default)]
        case_sensitive: bool,
    },
    ReplaceText {
        find: String,
        replace: String,
        #[serde(default)]
        case_sensitive: bool,
    },
    AddDateToFilename {
        #[serde(default)]
        placement: DatePlacement,
        #[serde(default = "default_date_format")]
        format: String,
    },
    ChangeExtension {
        new_extension: String,
        #[serde(default)]
        confirm_unsafe: bool,
    },
    CreateDestinationFolder {
        path: String,
    },
    Skip,
    StopProcessing,
    Notify {
        message: String,
    },
    MarkForReview {
        reason: String,
    },
    ApplyAnotherRule {
        rule_id: String,
    },
}

/// Pure snapshot of a file's attributes passed to the rule evaluator and template engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileFacts {
    pub path: String,
    pub name: String,
    pub stem: String,
    pub extension: String,
    pub mime: Option<String>,
    pub category: String,
    #[specta(type = f64)]
    pub size: u64,
    #[specta(type = Option<f64>)]
    pub created_ms: Option<i64>,
    #[specta(type = f64)]
    pub modified_ms: i64,
    #[specta(type = Option<f64>)]
    pub added_ms: Option<i64>,
    pub is_hidden: bool,
    pub is_duplicate: bool,
    pub content_hash: Option<String>,
    pub source_folder_id: String,
    pub source_folder_path: String,
    pub source_folder_name: String,
}

impl FileFacts {
    /// Splits a filename into `(stem, extension)` using macOS/Unix conventions:
    /// a leading dot on a single-dot hidden file (`.zshrc`) is part of the stem,
    /// not an extension.
    pub fn split_name(name: &str) -> (String, String) {
        let p = Path::new(name);
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(name)
            .to_owned();
        (stem, ext)
    }

    pub fn parent_folder_name(&self) -> String {
        Path::new(&self.path)
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub priority: i32,
    pub source_folders: Vec<String>,
    pub conditions: ConditionNode,
    pub actions: Vec<Action>,
    #[serde(default)]
    pub date_source: DateSource,
    pub execution_mode: ExecutionMode,
    pub origin: RuleOrigin,
    #[specta(type = Option<f64>)]
    pub last_run_at: Option<i64>,
    #[specta(type = i32)]
    pub stat_matched: i64,
    #[specta(type = i32)]
    pub stat_succeeded: i64,
    #[specta(type = i32)]
    pub stat_failed: i64,
    #[specta(type = f64)]
    pub created_at: i64,
    #[specta(type = f64)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RuleDraft {
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub source_folders: Vec<String>,
    pub conditions: ConditionNode,
    pub actions: Vec<Action>,
    #[serde(default)]
    pub date_source: DateSource,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConditionsEnvelope {
    v: u32,
    root: ConditionNode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionsEnvelope {
    v: u32,
    items: Vec<Action>,
    #[serde(default)]
    date_source: DateSource,
}

pub fn encode_conditions_json(node: &ConditionNode) -> AppResult<String> {
    let env = ConditionsEnvelope {
        v: RULE_SCHEMA_VERSION,
        root: node.clone(),
    };
    Ok(serde_json::to_string(&env)?)
}

pub fn decode_conditions_json(raw: &str) -> AppResult<ConditionNode> {
    let env: ConditionsEnvelope = serde_json::from_str(raw)?;
    if env.v != RULE_SCHEMA_VERSION {
        return Err(AppError::validation(
            "conditions",
            format!("unsupported conditions schema version {}", env.v),
        ));
    }
    Ok(env.root)
}

pub fn encode_actions_json(actions: &[Action], date_source: DateSource) -> AppResult<String> {
    let env = ActionsEnvelope {
        v: RULE_SCHEMA_VERSION,
        items: actions.to_vec(),
        date_source,
    };
    Ok(serde_json::to_string(&env)?)
}

pub fn decode_actions_json(raw: &str) -> AppResult<(Vec<Action>, DateSource)> {
    let env: ActionsEnvelope = serde_json::from_str(raw)?;
    if env.v != RULE_SCHEMA_VERSION {
        return Err(AppError::validation(
            "actions",
            format!("unsupported actions schema version {}", env.v),
        ));
    }
    Ok((env.items, env.date_source))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tree() -> ConditionNode {
        ConditionNode::All {
            children: vec![
                ConditionNode::Leaf {
                    condition: Condition::ExtensionGroup {
                        extensions: vec!["pdf".into(), "png".into()],
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
                                op: TextOp::MatchesRegex,
                                value: "^INV-\\d+".into(),
                                case_sensitive: true,
                            },
                        },
                    ],
                },
                ConditionNode::Not {
                    child: Box::new(ConditionNode::Leaf {
                        condition: Condition::Hidden { is_hidden: true },
                    }),
                },
                ConditionNode::Leaf {
                    condition: Condition::Size {
                        op: SizeOp::Between,
                        value: 1,
                        max_value: Some(50),
                        unit: SizeUnit::Megabytes,
                    },
                },
                ConditionNode::Leaf {
                    condition: Condition::Date {
                        field: DateField::Modified,
                        op: DateConditionOp::NewerThanDays { days: 90 },
                    },
                },
                ConditionNode::Leaf {
                    condition: Condition::Location {
                        op: LocationOp::InsideSubdirectory {
                            subpath: Some("Receipts".into()),
                        },
                    },
                },
            ],
        }
    }

    #[test]
    fn r_rule_01_condition_tree_and_actions_serde_roundtrip() {
        let tree = sample_tree();
        let actions = vec![
            Action::MoveIntoDynamicFolder {
                root: None,
                subpath_template: "Invoices/{year}/{month_name}".into(),
            },
            Action::Rename {
                template: "{date}_{original_name}".into(),
            },
            Action::AddPrefix {
                prefix: "paid_".into(),
            },
            Action::AddSuffix {
                suffix: "_verified".into(),
            },
            Action::RemoveText {
                text: "draft_".into(),
                case_sensitive: false,
            },
            Action::ReplaceText {
                find: " ".into(),
                replace: "_".into(),
                case_sensitive: false,
            },
            Action::AddDateToFilename {
                placement: DatePlacement::Prefix,
                format: "{date}".into(),
            },
            Action::ChangeExtension {
                new_extension: "pdf".into(),
                confirm_unsafe: true,
            },
            Action::CreateDestinationFolder {
                path: "/Users/alice/Documents/Invoices".into(),
            },
            Action::Copy {
                destination: "/Users/alice/Documents/Backup".into(),
            },
            Action::Move {
                destination: "/Users/alice/Documents/Final".into(),
            },
            Action::Notify {
                message: "Filed invoice".into(),
            },
            Action::MarkForReview {
                reason: "Check amount".into(),
            },
            Action::ApplyAnotherRule {
                rule_id: "rule-2".into(),
            },
            Action::Skip,
            Action::StopProcessing,
        ];

        let cond_json = encode_conditions_json(&tree).expect("encode conditions");
        let decoded_tree = decode_conditions_json(&cond_json).expect("decode conditions");
        assert_eq!(decoded_tree, tree);

        let act_json = encode_actions_json(&actions, DateSource::Created).expect("encode actions");
        let (decoded_actions, decoded_source) =
            decode_actions_json(&act_json).expect("decode actions");
        assert_eq!(decoded_actions, actions);
        assert_eq!(decoded_source, DateSource::Created);
    }

    #[test]
    fn r_rule_02_versioned_json_envelopes_enforce_schema_version() {
        let tree = sample_tree();
        let cond_json = encode_conditions_json(&tree).expect("encode");
        let raw_val: serde_json::Value = serde_json::from_str(&cond_json).expect("parse json");
        assert_eq!(raw_val["v"], RULE_SCHEMA_VERSION);

        let act_json =
            encode_actions_json(&[Action::Skip], DateSource::Modified).expect("encode actions");
        let act_val: serde_json::Value = serde_json::from_str(&act_json).expect("parse json");
        assert_eq!(act_val["v"], RULE_SCHEMA_VERSION);

        let future_cond = r#"{"v":99,"root":{"type":"all","children":[]}}"#;
        assert!(decode_conditions_json(future_cond).is_err());

        let future_act = r#"{"v":99,"items":[],"dateSource":"modified"}"#;
        assert!(decode_actions_json(future_act).is_err());
    }

    #[test]
    fn r_rule_03_execution_modes_and_origins_match_db_constraints_and_no_shell_action_exists() {
        for mode in [
            ExecutionMode::FirstMatch,
            ExecutionMode::Continue,
            ExecutionMode::StopAfterMove,
            ExecutionMode::StopAfterAction,
            ExecutionMode::Sequential,
        ] {
            let db_str = mode.as_db();
            assert_eq!(ExecutionMode::from_db(db_str).expect("from_db"), mode);
        }
        assert!(ExecutionMode::from_db("invalid_mode").is_err());

        for origin in [RuleOrigin::User, RuleOrigin::Preset, RuleOrigin::Import] {
            let db_str = origin.as_db();
            assert_eq!(RuleOrigin::from_db(db_str).expect("from_db"), origin);
        }
        assert!(RuleOrigin::from_db("cloud").is_err());

        // FR-051: Any attempt to deserialize a shell/script/command action must fail.
        for forbidden in [
            r#"{"kind":"shell","command":"rm -rf /"}"#,
            r#"{"kind":"run_script","path":"/tmp/x.sh"}"#,
            r#"{"kind":"exec","args":["sh"]}"#,
        ] {
            assert!(
                serde_json::from_str::<Action>(forbidden).is_err(),
                "forbidden action deserialized: {forbidden}"
            );
        }
    }
}
