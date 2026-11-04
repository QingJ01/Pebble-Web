import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import WebhooksTab from "../../../src/features/settings/WebhooksTab";
import { createWebhook, listWebhooks, testWebhook, updateWebhook } from "../../../src/lib/api";
import type { WebhookEndpoint } from "../../../src/lib/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string | Record<string, unknown>) => (typeof fallback === "string" ? fallback : key),
  }),
}));
vi.mock("../../../src/lib/api", () => ({
  listWebhooks: vi.fn(),
  createWebhook: vi.fn(),
  updateWebhook: vi.fn(),
  deleteWebhook: vi.fn(),
  testWebhook: vi.fn(),
  testSavedWebhook: vi.fn(),
}));

const saved: WebhookEndpoint = {
  id: "wh1",
  name: "Team ntfy",
  provider: "ntfy",
  is_enabled: true,
  notify_on_new_mail: true,
  created_at: 1,
  updated_at: 1,
};

describe("WebhooksTab", () => {
  afterEach(cleanup);
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(listWebhooks).mockResolvedValue([]);
  });

  it("creates a Feishu webhook with only the fields for that provider", async () => {
    vi.mocked(createWebhook).mockResolvedValue({ ...saved, provider: "feishu" });
    render(<WebhooksTab />);
    fireEvent.click(await screen.findByRole("button", { name: "Add webhook" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: " Alerts " } });
    fireEvent.change(screen.getByLabelText("Provider"), { target: { value: "feishu" } });
    fireEvent.change(screen.getByLabelText("Webhook URL"), { target: { value: " https://open.feishu.cn/hook " } });
    fireEvent.change(screen.getByLabelText("Signing secret"), { target: { value: "sig" } });
    fireEvent.click(screen.getByLabelText("Notify on new mail"));
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(createWebhook).toHaveBeenCalledWith(
      "Alerts",
      "feishu",
      { url: "https://open.feishu.cn/hook", secret: "sig" },
      true,
      true,
    ));
  });

  it("edits a saved webhook without resending or exposing its secrets", async () => {
    vi.mocked(listWebhooks).mockResolvedValue([saved]);
    vi.mocked(updateWebhook).mockResolvedValue();
    vi.mocked(testWebhook).mockResolvedValue();
    render(<WebhooksTab />);
    fireEvent.click(await screen.findByText("Team ntfy"));
    const token = screen.getByLabelText("Token") as HTMLInputElement;
    expect(token.value).toBe("");
    expect(token.placeholder).toBe("Saved — leave blank to keep");
    const priority = screen.getByLabelText("Priority") as HTMLInputElement;
    expect(priority.value).toBe("");
    expect(priority.placeholder).toBe("Saved — leave blank to keep");
    fireEvent.click(screen.getAllByRole("button", { name: "Test" })[0]);
    await waitFor(() => expect(testWebhook).toHaveBeenCalledWith("ntfy", {}, "wh1"));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Renamed" } });
    fireEvent.click(screen.getByLabelText("Notify on new mail"));
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(updateWebhook).toHaveBeenCalledWith(
      { id: "wh1", name: "Renamed", provider: "ntfy", is_enabled: true, notify_on_new_mail: false },
      {},
    ));
  });

  it("sends an explicitly changed priority when editing", async () => {
    vi.mocked(listWebhooks).mockResolvedValue([saved]);
    vi.mocked(updateWebhook).mockResolvedValue();
    render(<WebhooksTab />);
    fireEvent.click(await screen.findByText("Team ntfy"));
    fireEvent.change(screen.getByLabelText("Priority"), { target: { value: "2" } });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(updateWebhook).toHaveBeenCalledWith(
      expect.objectContaining({ id: "wh1" }),
      { priority: 2 },
    ));
  });
});
