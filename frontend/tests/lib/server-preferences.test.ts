import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/web-commands";
import { applyServerPreference, invalidatePreferenceRequests, reloadServerPreference } from "@/lib/server-preferences";
import { useUIStore } from "@/stores/ui.store";

vi.mock("@/lib/web-commands", () => ({ invoke: vi.fn() }));

describe("server preference ordering", () => {
  beforeEach(() => {
    invalidatePreferenceRequests();
    vi.mocked(invoke).mockReset();
    useUIStore.setState({ realtimeMode: "balanced", serverPreferencesReady: true });
  });

  it("does not replace a newer server event with an earlier request", async () => {
    let finish!: (value: string) => void;
    vi.mocked(invoke).mockReturnValue(new Promise(resolve => { finish = resolve; }));
    const pending = reloadServerPreference();
    applyServerPreference("manual");
    finish("battery");
    await pending;
    expect(useUIStore.getState().realtimeMode).toBe("manual");
  });

  it("fails closed on the current request but ignores stale failure readiness changes", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("unavailable"));
    await expect(reloadServerPreference()).rejects.toThrow("unavailable");
    expect(useUIStore.getState().serverPreferencesReady).toBe(false);
    let fail!: (error: Error) => void;
    vi.mocked(invoke).mockReturnValue(new Promise((_resolve, reject) => { fail = reject; }));
    const pending = reloadServerPreference();
    applyServerPreference("realtime");
    fail(new Error("stale"));
    await expect(pending).rejects.toThrow("stale");
    expect(useUIStore.getState().serverPreferencesReady).toBe(true);
  });
});
