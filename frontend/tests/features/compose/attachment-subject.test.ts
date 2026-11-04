import { describe, expect, it } from "vitest";
import { subjectAfterAddingAttachments } from "../../../src/features/compose/attachment-subject";

describe("subjectAfterAddingAttachments", () => {
  it("uses the first new attachment filename for a blank subject", () => {
    expect(subjectAfterAddingAttachments("", 0, ["季度报告.pdf", "supporting-data.xlsx"]))
      .toBe("季度报告.pdf");
  });

  it("treats a whitespace-only subject as blank", () => {
    expect(subjectAfterAddingAttachments("   ", 0, ["meeting-notes.docx"]))
      .toBe("meeting-notes.docx");
  });

  it("preserves a subject already entered by the user", () => {
    expect(subjectAfterAddingAttachments("Project update", 0, ["report.pdf"]))
      .toBe("Project update");
  });

  it("does not derive a subject from a later attachment", () => {
    expect(subjectAfterAddingAttachments("", 1, ["second-file.pdf"]))
      .toBe("");
  });
});
