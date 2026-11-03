import { describe, expect, it } from "vitest";
import {
  defaultPrivacyMode,
  privacyModeToApi,
  readStoredPrivacyMode,
} from "../../src/lib/privacyMode";

describe("privacy mode defaults", () => {
  it("defaults to relaxed when no stored preference exists", () => {
    expect(readStoredPrivacyMode({ getItem: () => null })).toBe("relaxed");
    expect(defaultPrivacyMode()).toBe("LoadOnce");
  });

  it("falls back to relaxed for unknown stored values", () => {
    expect(readStoredPrivacyMode({ getItem: () => "unknown" })).toBe("relaxed");
  });

  it("maps stored privacy modes to render API modes", () => {
    expect(privacyModeToApi("strict")).toBe("Strict");
    expect(privacyModeToApi("relaxed")).toBe("LoadOnce");
    expect(privacyModeToApi("off")).toBe("Off");
  });
});
