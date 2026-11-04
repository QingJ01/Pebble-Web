//! Persisted rule schema — MUST stay byte-compatible with the frontend
//! types in `src/features/settings/rule-json.ts`. The rules table stores
//! the JSON serialization of these types, and the settings UI reads/writes
//! the same strings. A rename or variant change on one side without the
//! other will silently drop rules at load. Fix both sides together; a
//! shared ts-rs-generated binding is tracked as future work.

use pebble_core::KanbanColumn;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConditionSet {
    pub operator: LogicalOp,
    pub conditions: Vec<RuleCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleCondition {
    pub field: ConditionField,
    pub op: ConditionOp,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionField {
    From,
    To,
    Subject,
    Body,
    HasAttachment,
    Domain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Contains,
    NotContains,
    Equals,
    StartsWith,
    EndsWith,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum RuleAction {
    AddLabel(String),
    MoveToFolder(String),
    MarkRead,
    Archive,
    SetKanbanColumn(KanbanColumn),
}
