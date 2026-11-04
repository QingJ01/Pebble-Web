import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { requestBrowserNotifications, showMailNotification } from "@/lib/browser-notifications";
import { useUIStore } from "@/stores/ui.store";

vi.mock("@/lib/i18n", () => ({ default: { t: (_key: string, fallback: string) => fallback } }));

class BrowserNotification {
  static permission: NotificationPermission = "granted";
  static requestPermission = vi.fn();
  static created: NotificationOptions[] = [];
  onclick: (() => void) | null = null;
  close = vi.fn();
  constructor(_title: string, options: NotificationOptions) { BrowserNotification.created.push(options); }
}

describe("opt-in browser notifications", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.stubGlobal("Notification", BrowserNotification);
    vi.stubGlobal("isSecureContext", true);
    BrowserNotification.permission = "granted";
    BrowserNotification.requestPermission.mockReset();
    BrowserNotification.created = [];
    useUIStore.setState({ notificationsEnabled: true });
  });
  afterEach(() => { vi.unstubAllGlobals(); localStorage.clear(); });

  it("reports denied permission and never requests it from an incoming-mail event", async () => {
    BrowserNotification.permission = "default";
    BrowserNotification.requestPermission.mockResolvedValue("denied");
    await showMailNotification("mail:new", { message_id: "one", subject: "Hello" });
    expect(BrowserNotification.requestPermission).not.toHaveBeenCalled();
    expect(BrowserNotification.created).toHaveLength(0);
    await expect(requestBrowserNotifications()).rejects.toThrow("Allow notifications");
    expect(BrowserNotification.requestPermission).toHaveBeenCalledOnce();
  });

  it("deduplicates repeated events and honors the user's disabled preference", async () => {
    await showMailNotification("mail:new", { message_id: "one", subject: "Hello" });
    await showMailNotification("mail:new", { message_id: "one", subject: "Hello" });
    expect(BrowserNotification.created).toEqual([{ body: "Hello", tag: "pebble:mail:new:one" }]);
    useUIStore.setState({ notificationsEnabled: false });
    await showMailNotification("mail:new", { message_id: "two", subject: "Other" });
    expect(BrowserNotification.created).toHaveLength(1);
  });
});
