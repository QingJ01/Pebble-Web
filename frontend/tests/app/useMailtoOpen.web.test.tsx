import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { openMailtoUrl, useMailtoOpen } from "@/app/useMailtoOpen";
import { useComposeStore } from "@/stores/compose.store";
import { useUIStore } from "@/stores/ui.store";

const confirm = vi.hoisted(() => vi.fn());
vi.mock("@/stores/confirm.store", () => ({ useConfirmStore: { getState: () => ({ confirm }) } }));

describe("browser mailto handling", () => {
  beforeEach(() => {
    useUIStore.setState({ activeView: "inbox" });
    useComposeStore.setState({ composeMode: null, composeDirty: false, composePrefill: null });
    confirm.mockReset();
  });

  it("intercepts in-app mail links and preserves encoded recipients/CC/BCC", async () => {
    const { unmount } = renderHook(() => useMailtoOpen());
    const anchor = document.createElement("a");
    anchor.href = "mailto:a%40example.com,b%40example.com?cc=c%40example.com&bcc=d%40example.com&subject=Hello%20Web";
    document.body.appendChild(anchor);
    await act(async () => { anchor.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true })); });
    expect(useComposeStore.getState().composePrefill).toMatchObject({
      to: ["a@example.com", "b@example.com"], cc: ["c@example.com"], bcc: ["d@example.com"], subject: "Hello Web",
    });
    expect(confirm).not.toHaveBeenCalled();
    anchor.remove();
    unmount();
  });

  it("keeps an unsaved compose when replacement is cancelled", async () => {
    useUIStore.setState({ activeView: "compose" });
    useComposeStore.setState({ composeMode: "new", composeDirty: true, composePrefill: { subject: "Keep me" } });
    confirm.mockResolvedValue(false);
    await expect(openMailtoUrl("mailto:new@example.com")).resolves.toBe(false);
    expect(confirm).toHaveBeenCalledOnce();
    expect(useComposeStore.getState().composePrefill?.subject).toBe("Keep me");
  });
});
