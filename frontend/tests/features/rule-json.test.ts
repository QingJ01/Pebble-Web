import { describe, expect, it } from "vitest";
import {
  parseRuleActions,
  parseRuleConditions,
  serializeRuleActions,
  serializeRuleConditions,
  type RuleActionInput,
  type RuleConditionInput,
} from "../../src/features/settings/rule-json";

describe("rule-json", () => {
  it("serializes conditions into the backend condition-set contract", () => {
    const conditions: RuleConditionInput[] = [
      { field: "from", op: "contains", value: "newsletter" },
    ];

    expect(serializeRuleConditions(conditions)).toBe(
      JSON.stringify({
        operator: "and",
        conditions,
      }),
    );
  });

  it("serializes actions into tagged enum objects", () => {
    const actions: RuleActionInput[] = [
      { type: "AddLabel", value: "newsletters" },
      { type: "MarkRead" },
      { type: "SetKanbanColumn", value: "todo" },
    ];

    expect(serializeRuleActions(actions)).toBe(
      JSON.stringify([
        { type: "AddLabel", value: "newsletters" },
        { type: "MarkRead" },
        { type: "SetKanbanColumn", value: "todo" },
      ]),
    );
  });

  it("parses backend condition-set payloads back into the editor model", () => {
    const parsed = parseRuleConditions(
      JSON.stringify({
        operator: "and",
        conditions: [{ field: "subject", op: "contains", value: "invoice" }],
      }),
    );

    expect(parsed).toEqual([
      { field: "subject", op: "contains", value: "invoice" },
    ]);
  });

  it("parses tagged action payloads back into the editor model", () => {
    const parsed = parseRuleActions(
      JSON.stringify([
        { type: "MoveToFolder", value: "Archive" },
        { type: "MarkRead" },
      ]),
    );

    expect(parsed).toEqual([
      { type: "MoveToFolder", value: "Archive" },
      { type: "MarkRead" },
    ]);
  });
});
