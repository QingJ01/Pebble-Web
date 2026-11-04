import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { readAppLog, type AppLogSnapshot } from "@/lib/api";
import { invoke } from "@/lib/web-commands";
import iconUrl from "@/assets/app-icon.png";

export default function AboutTab() {
  const { t } = useTranslation();
  const [version, setVersion] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<AppLogSnapshot | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    let active = true;
    invoke<{ version: string }>("get_server_info").then(info => { if (active) setVersion(info.version); }).catch(() => {});
    return () => { active = false; };
  }, []);
  async function loadLog() {
    setLoading(true);
    setError("");
    try { setSnapshot(await readAppLog(64 * 1024)); }
    catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { setLoading(false); }
  }
  async function copyLog() {
    try { await navigator.clipboard.writeText(snapshot?.content ?? ""); }
    catch { setError(t("web.clipboardUnavailable", "Clipboard access was denied. Select and copy the log text manually.")); }
  }
  return <section style={{ fontSize: 13, lineHeight: 1.7, color: "var(--color-text-primary)" }}>
    <h2 style={{ display: "flex", alignItems: "center", gap: 12 }}><img src={iconUrl} alt="" width={48} height={48} /> Pebble Web</h2>
    <p>{t("about.version", "Version")}: {version ?? t("common.loading", "Loading...")}</p>
    <p>{t("web.aboutDescription", "A self-hosted email client. Mail, attachments and search indexes are stored on your Pebble server, not only in this browser. Translation and WebDAV contact the services you configure.")}</p>
    <p>{t("about.license", "Open source under AGPL-3.0 license.")}</p>
    <h3>{t("about.updates", "Updates")}</h3>
    <p>{t("web.deploymentUpdates", "Update the server or Docker image to upgrade Pebble Web. The browser cannot install desktop updates, manage startup services, or create a system tray icon.")}</p>
    <a href="https://github.com/QingJ01/Pebble-Web/releases" target="_blank" rel="noopener noreferrer">{t("web.webReleases", "Web releases")}</a>
    <h3>{t("about.openDiagnosticLog", "Open diagnostic log")}</h3>
    <button type="button" onClick={loadLog} disabled={loading}>{loading ? t("common.loading", "Loading...") : t("web.loadLogs", "Load server diagnostics")}</button>
    {error && <p role="alert" style={{ color: "var(--color-danger, #dc2626)" }}>{error}</p>}
    {snapshot && <div>
      <p>{snapshot.truncated && t("web.logsTruncated", "Showing the latest log entries only.")}</p>
      <button type="button" onClick={copyLog}>{t("common.copy", "Copy")}</button>
      <pre tabIndex={0} style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere", maxHeight: 420, overflow: "auto", fontSize: 11, padding: 12, background: "var(--color-bg-secondary)" }}>{snapshot.content || t("web.emptyLogs", "No log entries available.")}</pre>
    </div>}
  </section>;
}
