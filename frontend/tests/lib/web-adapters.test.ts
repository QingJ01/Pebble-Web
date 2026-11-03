import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import http from "@/api-client";
import { invoke } from "@/lib/web-commands";
import { completeWebOAuth } from "@/lib/web-oauth";
import { backgroundImageUrl, importBackgroundImage } from "@/lib/backgroundImage";
import { downloadAttachment } from "@/lib/api";

vi.mock("@/api-client", () => ({ default: { post: vi.fn(), get: vi.fn() } }));

describe("authenticated browser adapters", () => {
  beforeEach(() => { vi.mocked(http.post).mockReset(); vi.mocked(http.get).mockReset(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it("uses the typed command bridge and preserves arguments/results", async () => {
    vi.mocked(http.post).mockResolvedValue({ data: { id: "account" } });
    await expect(invoke("get_account_sync_range", { accountId: "account" })).resolves.toEqual({ id: "account" });
    expect(http.post).toHaveBeenCalledWith("/commands/get_account_sync_range", { accountId: "account" });
  });

  it("surfaces backend validation failures rather than success-shaped defaults", async () => {
    vi.mocked(http.post).mockRejectedValue({ response: { data: { error: "POP3 does not support a bounded range" } } });
    await expect(invoke("update_account_sync_range", { days: 30 })).rejects.toThrow("POP3");
  });

  it("fails before starting OAuth when the popup is blocked", async () => {
    vi.spyOn(window, "open").mockReturnValue(null);
    await expect(completeWebOAuth("gmail", {})).rejects.toThrow("Allow popups");
    expect(http.post).not.toHaveBeenCalled();
  });

  it("rejects a non-provider authorization URL and closes the popup", async () => {
    const popup = { opener: window, location: { replace: vi.fn() }, close: vi.fn() };
    vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
    vi.mocked(http.post).mockResolvedValue({ data: { authorization_url: "https://attacker.example/", flow_id: "flow" } });
    await expect(completeWebOAuth("gmail", {})).rejects.toThrow("invalid provider");
    expect(popup.location.replace).not.toHaveBeenCalled();
    expect(popup.close).toHaveBeenCalledOnce();
  });

  it("polls the initiating secret flow and returns only the verified account", async () => {
    const popup = { opener: window, closed: false, location: { replace: vi.fn() }, close: vi.fn() };
    vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
    vi.mocked(http.post).mockResolvedValue({ data: { authorization_url: "https://accounts.google.com/o/oauth2/v2/auth", flow_id: "secret-flow" } });
    vi.mocked(http.get).mockResolvedValue({ data: { status: "completed", account: { id: "verified" } } });
    await expect(completeWebOAuth("gmail", { syncRangeDays: 30 })).resolves.toEqual({ id: "verified" });
    expect(http.post).toHaveBeenCalledWith("/oauth/gmail/start", { syncRangeDays: 30 }, { timeout: 15000 });
    expect(http.get).toHaveBeenCalledWith("/oauth/gmail/status/secret-flow", { timeout: 15000 });
    expect(popup.opener).toBeNull();
    expect(popup.close).toHaveBeenCalledOnce();
  });

  it("downloads authenticated bytes without accepting a server destination path", async () => {
    vi.mocked(http.get).mockResolvedValue({ data: new Blob(["test"]) });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    vi.stubGlobal("URL", Object.assign(URL, { createObjectURL: vi.fn(() => "blob:download"), revokeObjectURL: vi.fn() }));
    await expect(downloadAttachment("id/one", "报告.pdf")).resolves.toBe("报告.pdf");
    expect(http.get).toHaveBeenCalledWith("/attachments/id%2Fone/download", { responseType: "blob" });
    expect(click).toHaveBeenCalledOnce();
    expect(document.querySelector("a[download]")).toBeNull();
  });

  it("persists wallpaper bytes rather than a document-lifetime blob URL", async () => {
    const image = await importBackgroundImage(new File(["png-bytes"], "wallpaper.png", { type: "image/png" }));
    expect(image.path).toMatch(/^data:image\/png;base64,/);
    const restored = JSON.parse(JSON.stringify(image));
    expect(backgroundImageUrl(restored.path)).toBe(image.path);
    expect(backgroundImageUrl("https://tracking.example/image.png")).toBe("");
    await expect(importBackgroundImage(new File(["<svg/>"], "unsafe.svg", { type: "image/svg+xml" }))).rejects.toThrow("PNG");
  });
});
