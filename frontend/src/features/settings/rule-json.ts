// Rule JSON schema — MUST stay byte-compatible with the backend types in
// `crates/pebble-rules/src/types.rs`. Both sides (de)serialize the same
// persisted strings, so adding, removing, or renaming a variant here without
// a matching change there (and vice versa) will silently skip or reject
// rules at load time. When the two drift, prefer fixing both; a future
// migration to shared ts-rs-generated bindings is tracked separately.
export type ConditionField =
  | "from"
  | "to"
  | "subject"
  | "body"
  | "has_attachment"
  | "domain";

export type ConditionOp =
  | "contains"
  | "not_contains"
  | "equals"
  | "starts_with"
  | "ends_with";

export interface RuleConditionInput {
  field: ConditionField;
  op: ConditionOp;
  value: string;
}

export type ActionType =
  | "AddLabel"
  | "MoveToFolder"
  | "MarkRead"
  | "Archive"
  | "SetKanbanColumn"
  | "SendWebhook";

export type KanbanColumn = "todo" | "waiting" | "done";

export interface RuleActionInput {
  type: ActionType;
  value?: string;
}

const DEFAULT_CONDITION: RuleConditionInput = {
  field: "from",
  op: "contains",
  value: "",
};

const DEFAULT_ACTION: RuleActionInput = { type: "Archive" };

export function parseRuleConditions(json: string): RuleConditionInput[] {
  try {
    const parsed = JSON.parse(json);

    if (Array.isArray(parsed)) {
      return parsed as RuleConditionInput[];
    }

    if (parsed && Array.isArray(parsed.conditions)) {
      return parsed.conditions as RuleConditionInput[];
    }
  } catch {
    // fall through to default
  }

  return [DEFAULT_CONDITION];
}

export function serializeRuleConditions(
  conditions: RuleConditionInput[],
): string {
  return JSON.stringify({
    operator: "and",
    conditions,
  });
}

export function parseRuleActions(json: string): RuleActionInput[] {
  try {
    const parsed = JSON.parse(json);
    if (!Array.isArray(parsed)) {
      return [DEFAULT_ACTION];
    }

    return parsed.map((action: unknown) => {
      if (typeof action === "string") {
        return { type: action as ActionType };
      }

      if (!action || typeof action !== "object") {
        return DEFAULT_ACTION;
      }

      const record = action as Record<string, unknown>;

      if (typeof record.type === "string") {
        return {
          type: record.type as ActionType,
          value:
            typeof record.value === "string" ? record.value : undefined,
        };
      }

      if ("AddLabel" in record) {
        return { type: "AddLabel", value: String(record.AddLabel ?? "") };
      }

      if ("MoveToFolder" in record) {
        return {
          type: "MoveToFolder",
          value: String(record.MoveToFolder ?? ""),
        };
      }

      if ("SetKanbanColumn" in record) {
        return {
          type: "SetKanbanColumn",
          value: String(record.SetKanbanColumn ?? "todo"),
        };
      }

      const [legacyType = "Archive", legacyValue] = Object.entries(record)[0] || [];
      return {
        type: legacyType as ActionType,
        value: legacyValue === undefined ? undefined : String(legacyValue),
      };
    });
  } catch {
    return [DEFAULT_ACTION];
  }
}

export function serializeRuleActions(actions: RuleActionInput[]): string {
  return JSON.stringify(
    actions.map((action) => {
      if (action.type === "MarkRead" || action.type === "Archive") {
        return { type: action.type };
      }

      if (action.type === "SetKanbanColumn") {
        return { type: action.type, value: action.value || "todo" };
      }

      return { type: action.type, value: action.value || "" };
    }),
  );
}
