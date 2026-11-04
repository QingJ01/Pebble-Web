import { describe, expect, it } from "vitest";
import { sanitizeFilename } from "../../src/lib/sanitizeFilename";

describe("sanitizeFilename", () => {
  it("replaces Windows-unsafe characters and trims trailing dots", () => {
    expect(sanitizeFilename("quarterly:report*final?.pdf")).toBe(
      "quarterly_report_final_.pdf",
    );
    expect(sanitizeFilename("report. ")).toBe("report");
  });

  it("rejects reserved Windows device names", () => {
    expect(sanitizeFilename("CON.txt")).toBe("download");
    expect(sanitizeFilename("LPT1.log")).toBe("download");
  });
});
