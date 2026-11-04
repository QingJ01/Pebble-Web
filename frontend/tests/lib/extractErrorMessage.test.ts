import { describe, expect, it } from "vitest";
import { extractErrorMessage } from "../../src/lib/extractErrorMessage";

describe("extractErrorMessage", () => {
  it("prefers the server's error body over the generic HTTP message", () => {
    const err = Object.assign(new Error("Request failed with status code 400"), {
      response: { data: { error: "Webhook returned HTTP 403 Forbidden" } },
    });
    expect(extractErrorMessage(err)).toBe("Webhook returned HTTP 403 Forbidden");
  });

  it("falls back to the error message", () => {
    expect(extractErrorMessage(new Error("boom"))).toBe("boom");
  });
});
