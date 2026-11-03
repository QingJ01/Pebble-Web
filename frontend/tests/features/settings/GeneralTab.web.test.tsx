import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import GeneralTab from "@/features/settings/GeneralTab";
import { useUIStore } from "@/stores/ui.store";
import { setRealtimePreference } from "@/lib/api";

vi.mock("@/lib/api", () => ({ setRealtimePreference: vi.fn(), showTestNotification: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string, fallback?: string) => fallback ?? key }) }));

describe("Web server preferences", () => {
  beforeEach(() => { useUIStore.setState({ serverPreferencesReady: true, realtimeMode: "balanced" }); vi.mocked(setRealtimePreference).mockReset(); });

  it("saves a deployment-wide mode before updating local presentation", async () => {
    vi.mocked(setRealtimePreference).mockResolvedValue(undefined);
    render(<GeneralTab />);
    fireEvent.click(screen.getByRole("button", { name: "web.syncMode.manual" }));
    await waitFor(() => expect(setRealtimePreference).toHaveBeenCalledWith("manual"));
    await waitFor(() => expect(useUIStore.getState().realtimeMode).toBe("manual"));
    expect(screen.queryByText(/Quit app when window/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/Start hidden/i)).not.toBeInTheDocument();
  });

  it("does not overwrite the server with a browser default before loading", () => {
    useUIStore.setState({ serverPreferencesReady: false });
    render(<GeneralTab />);
    expect(screen.getByRole("button", { name: "web.syncMode.manual" })).toBeDisabled();
    expect(setRealtimePreference).not.toHaveBeenCalled();
  });
});
