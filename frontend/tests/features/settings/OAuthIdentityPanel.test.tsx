import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import OAuthIdentityPanel from "../../../src/features/settings/OAuthIdentityPanel";
import { applyOAuthIdentity, previewOAuthIdentity } from "../../../src/lib/api";
import type { Account } from "../../../src/lib/ipc-types";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string, fallback?: string) => fallback ?? key }) }));
vi.mock("../../../src/lib/api", () => ({ previewOAuthIdentity: vi.fn(), applyOAuthIdentity: vi.fn() }));
const account: Account = { id: "a", email: "login@qq.com", display_name: "Legacy name", provider: "outlook", created_at: 1, updated_at: 1 };
const preview = { account_id: "a", previous_email: account.email, identity: { subject: "outlook:stable", email: "mailbox@outlook.com", display_name: "Mailbox name" } };
describe("OAuth mailbox repair", () => {
  afterEach(cleanup);
  beforeEach(() => { vi.resetAllMocks(); });
  it("shows a preview and applies only after the user chooses the verified identity", async () => {
    vi.mocked(previewOAuthIdentity).mockResolvedValue(preview);
    const updated = { ...account, email: preview.identity.email, provider_display_name: preview.identity.display_name };
    vi.mocked(applyOAuthIdentity).mockResolvedValue(updated);
    const onApplied = vi.fn();
    render(<OAuthIdentityPanel account={account} onApplied={onApplied} />);
    fireEvent.click(screen.getByRole("button", { name: "Verify mailbox" }));
    await screen.findByText("login@qq.com → mailbox@outlook.com");
    expect(applyOAuthIdentity).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Use verified mailbox details" }));
    await waitFor(() => expect(onApplied).toHaveBeenCalledWith(updated));
    expect(applyOAuthIdentity).toHaveBeenCalledWith(preview);
  });
  it("keeps the account unchanged when verification fails", async () => {
    vi.mocked(previewOAuthIdentity).mockRejectedValue(new Error("Mailbox unavailable"));
    const onApplied = vi.fn();
    render(<OAuthIdentityPanel account={account} onApplied={onApplied} />);
    fireEvent.click(screen.getByRole("button", { name: "Verify mailbox" }));
    await screen.findByRole("alert");
    expect(applyOAuthIdentity).not.toHaveBeenCalled();
    expect(onApplied).not.toHaveBeenCalled();
  });
});
