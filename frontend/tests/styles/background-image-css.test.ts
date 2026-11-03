import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

describe("background image CSS", () => {
  it("keeps the main content readable when imported backgrounds are visible", () => {
    const css = readFileSync(join(process.cwd(), "src", "styles", "index.css"), "utf8");

    expect(css).toMatch(
      /\.app-shell--with-background\s+\.app-main-scroll\s*\{[^}]*color-mix\(in srgb,\s*var\(--color-main-bg\)\s*68%,\s*transparent\)/i,
    );
    expect(css).toMatch(
      /\.app-shell--with-background\s+\.search-toolbar\s*\{[^}]*color-mix\(in srgb,\s*var\(--color-bg\)\s*12%,\s*transparent\)/i,
    );
    expect(css).toMatch(
      /\.app-shell--with-background\s+\.inbox-toolbar-row\s*\{[^}]*color-mix\(in srgb,\s*var\(--color-bg\)\s*12%,\s*transparent\)/i,
    );
  });
});
