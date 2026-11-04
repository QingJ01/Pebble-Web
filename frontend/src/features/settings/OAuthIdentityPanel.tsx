import { useState } from "react";
import { useTranslation } from "react-i18next";
import { applyOAuthIdentity, previewOAuthIdentity } from "@/lib/api";
import type { OAuthIdentityPreview } from "@/lib/api";
import type { Account } from "@/lib/ipc-types";
import { extractErrorMessage } from "@/lib/extractErrorMessage";

export default function OAuthIdentityPanel({ account, onApplied }: {
  account: Account;
  onApplied: (account: Account) => void;
}) {
  const { t } = useTranslation();
  const [preview, setPreview] = useState<OAuthIdentityPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [applied, setApplied] = useState(false);

  async function verify() {
    setBusy(true); setError(null); setPreview(null); setApplied(false);
    try { setPreview(await previewOAuthIdentity(account.id)); }
    catch (err) { setError(extractErrorMessage(err)); }
    finally { setBusy(false); }
  }
  async function apply() {
    if (!preview) return;
    setBusy(true); setError(null);
    try {
      onApplied(await applyOAuthIdentity(preview));
      setPreview(null); setApplied(true);
    } catch (err) { setError(extractErrorMessage(err)); }
    finally { setBusy(false); }
  }

  return <div style={{ gridColumn: "1 / -1", fontSize: "12px", color: "var(--color-text-secondary)" }}>
    <p>{t("accountSetup.oauthAddressHelp", "The mailbox address comes from your provider. Verify it here if it shows a different login address.")}</p>
    <button type="button" disabled={busy} onClick={() => void verify()}>
      {t("accountSetup.verifyIdentity", "Verify mailbox")}
    </button>
    {preview && <div role="status" style={{ marginTop: "8px", overflowWrap: "anywhere" }}>
      <div>{preview.previous_email} → {preview.identity.email}</div>
      {preview.identity.display_name && <div>{preview.identity.display_name}</div>}
      <p>{t("accountSetup.identityRepairHelp", "This updates the mailbox details while keeping your existing mail and account label.")}</p>
      <button type="button" disabled={busy} onClick={() => void apply()}>{t("accountSetup.applyIdentity", "Use verified mailbox details")}</button>
    </div>}
    {applied && <p role="status">{t("accountSetup.identityApplied", "Mailbox details updated.")}</p>}
    {error && <p role="alert">{error}</p>}
  </div>;
}
