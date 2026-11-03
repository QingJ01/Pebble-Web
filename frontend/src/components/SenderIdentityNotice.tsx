import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useAccountsQuery } from "@/hooks/queries";
import { profileLocalStorage } from "@/lib/profileStorage";
import { senderIdentityLabel } from "@/lib/accountIdentity";
import { useUIStore } from "@/stores/ui.store";

const NOTICE_KEY = "pebble-sender-identity-notice-v1";

export default function SenderIdentityNotice() {
  const { data: accounts = [] } = useAccountsQuery();
  const { t } = useTranslation();
  const [dismissed, setDismissed] = useState(() => profileLocalStorage.getItem(NOTICE_KEY) === "seen");
  const senders = accounts.filter((account) => account.provider !== "outlook" && account.display_name.trim());
  if (dismissed || senders.length === 0) return null;
  return <aside aria-label={t("accountSetup.senderNoticeTitle", "Review your sender names")}
    style={{ padding: "10px 16px", borderBottom: "1px solid var(--color-border)", fontSize: "12px", background: "var(--color-bg-secondary)", color: "var(--color-text-primary)" }}>
    <p style={{ margin: "0 0 6px" }}>{t("accountSetup.senderNotice", "Sender names now appear in outgoing mail. Review these names, and use Account label for private account descriptions.")}</p>
    <details><summary>{t("accountSetup.senderPreview", "Preview sender identities")}</summary>
      <ul style={{ maxHeight: "120px", overflowY: "auto", overflowWrap: "anywhere" }}>
        {senders.map((account) => <li key={account.id}>{senderIdentityLabel(account)}</li>)}
      </ul>
    </details>
    <div style={{ display: "flex", gap: "12px", marginTop: "6px" }}>
      <button type="button" onClick={() => {
        useUIStore.getState().setSettingsTab("accounts");
        useUIStore.getState().setActiveView("settings");
      }}>{t("accountSetup.reviewNames", "Review in account settings")}</button>
      <button type="button" onClick={() => {
        profileLocalStorage.setItem(NOTICE_KEY, "seen");
        setDismissed(true);
      }}>{t("accountSetup.senderNoticeDismiss", "Got it")}</button>
    </div>
  </aside>;
}
