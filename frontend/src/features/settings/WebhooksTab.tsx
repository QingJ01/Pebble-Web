import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import {
  createWebhook,
  deleteWebhook,
  listWebhooks,
  testSavedWebhook,
  testWebhook,
  updateWebhook,
  type WebhookConfig,
  type WebhookEndpoint,
  type WebhookProvider,
} from "@/lib/api";
import { extractErrorMessage } from "@/lib/extractErrorMessage";
import { useToastStore } from "@/stores/toast.store";

const PROVIDERS: WebhookProvider[] = [
  "generic",
  "slack",
  "discord",
  "telegram",
  "feishu",
  "dingtalk",
  "wecom",
  "ntfy",
  "gotify",
];

const SECRET_PLACEHOLDER = "••••••••";

interface FormState {
  id: string | null;
  name: string;
  provider: WebhookProvider;
  isEnabled: boolean;
  notifyOnNewMail: boolean;
  url: string;
  botToken: string;
  chatId: string;
  secret: string;
  serverUrl: string;
  topic: string;
  token: string;
  priority: number;
}

const emptyForm: FormState = {
  id: null,
  name: "",
  provider: "generic",
  isEnabled: true,
  notifyOnNewMail: false,
  url: "",
  botToken: "",
  chatId: "",
  secret: "",
  serverUrl: "",
  topic: "",
  token: "",
  priority: 5,
};

const inputStyle: React.CSSProperties = {
  width: "100%",
  padding: "8px 10px",
  borderRadius: "6px",
  border: "1px solid var(--color-border)",
  backgroundColor: "var(--color-bg)",
  color: "var(--color-text-primary)",
  fontSize: "13px",
  boxSizing: "border-box",
};

const labelStyle: React.CSSProperties = {
  display: "block",
  fontSize: "12px",
  fontWeight: 500,
  color: "var(--color-text-secondary)",
  marginBottom: "4px",
};

const primaryButtonStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "6px",
  flexShrink: 0,
  padding: "7px 14px",
  borderRadius: "6px",
  border: "none",
  backgroundColor: "var(--color-accent)",
  color: "#fff",
  cursor: "pointer",
  fontSize: "13px",
  fontWeight: 600,
  lineHeight: 1.2,
  whiteSpace: "nowrap",
};

const secondaryButtonStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  padding: "7px 14px",
  borderRadius: "6px",
  border: "1px solid var(--color-border)",
  backgroundColor: "transparent",
  color: "var(--color-text-primary)",
  cursor: "pointer",
  fontSize: "13px",
  lineHeight: 1.2,
  whiteSpace: "nowrap",
};

const iconButtonStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  padding: "7px",
  borderRadius: "6px",
  border: "1px solid var(--color-border)",
  backgroundColor: "transparent",
  color: "#ef4444",
  cursor: "pointer",
};

export default function WebhooksTab() {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [endpoints, setEndpoints] = useState<WebhookEndpoint[]>([]);
  const [form, setForm] = useState<FormState>(emptyForm);
  const [editing, setEditing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void fetchEndpoints();
  }, []);

  async function fetchEndpoints() {
    try {
      setEndpoints(await listWebhooks());
    } catch (err) {
      setError(extractErrorMessage(err));
    }
  }

  function startCreate() {
    setForm(emptyForm);
    setEditing(true);
    setError(null);
  }

  function startEdit(endpoint: WebhookEndpoint) {
    setForm({
      ...emptyForm,
      id: endpoint.id,
      name: endpoint.name,
      provider: endpoint.provider,
      isEnabled: endpoint.is_enabled,
      notifyOnNewMail: endpoint.notify_on_new_mail,
      url: SECRET_PLACEHOLDER,
      botToken: SECRET_PLACEHOLDER,
      secret: SECRET_PLACEHOLDER,
      token: SECRET_PLACEHOLDER,
    });
    setEditing(true);
    setError(null);
  }

  function buildConfig(): WebhookConfig {
    const clean = (value: string) => value === SECRET_PLACEHOLDER ? undefined : value.trim();
    switch (form.provider) {
      case "telegram":
        return { botToken: clean(form.botToken), chatId: form.chatId.trim() };
      case "ntfy":
        return {
          url: clean(form.url),
          serverUrl: form.serverUrl.trim(),
          topic: form.topic.trim(),
          token: clean(form.token),
          priority: form.priority,
        };
      case "gotify":
        return { serverUrl: form.serverUrl.trim(), token: clean(form.token), priority: form.priority };
      case "feishu":
      case "dingtalk":
        return { url: clean(form.url), secret: clean(form.secret) };
      default:
        return { url: clean(form.url) };
    }
  }

  async function save() {
    if (!form.name.trim()) {
      setError(t("webhooks.nameRequired", "Webhook name is required"));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const config = buildConfig();
      if (form.id) {
        await updateWebhook({
          id: form.id,
          name: form.name.trim(),
          provider: form.provider,
          is_enabled: form.isEnabled,
          notify_on_new_mail: form.notifyOnNewMail,
          created_at: 0,
          updated_at: 0,
        }, config);
      } else {
        await createWebhook(form.name.trim(), form.provider, config, form.isEnabled, form.notifyOnNewMail);
      }
      setEditing(false);
      setForm(emptyForm);
      await fetchEndpoints();
      addToast({ message: t("webhooks.saved", "Webhook saved"), type: "success" });
    } catch (err) {
      setError(extractErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function runTest(endpoint?: WebhookEndpoint) {
    try {
      if (endpoint) {
        await testSavedWebhook(endpoint.id);
      } else {
        await testWebhook(form.provider, buildConfig(), form.id);
      }
      addToast({ message: t("webhooks.testSent", "Test notification sent"), type: "success" });
    } catch (err) {
      addToast({ message: t("webhooks.testFailed", { error: extractErrorMessage(err) }), type: "error" });
    }
  }

  async function remove(endpoint: WebhookEndpoint) {
    try {
      await deleteWebhook(endpoint.id);
      await fetchEndpoints();
    } catch (err) {
      addToast({ message: t("webhooks.deleteFailed", { error: extractErrorMessage(err) }), type: "error" });
    }
  }

  function renderProviderFields() {
    if (form.provider === "telegram") {
      return (
        <>
          {field("botToken", t("webhooks.botToken", "Bot token"), form.botToken, (v) => setForm({ ...form, botToken: v }), true)}
          {field("chatId", t("webhooks.chatId", "Chat ID"), form.chatId, (v) => setForm({ ...form, chatId: v }))}
        </>
      );
    }
    if (form.provider === "ntfy") {
      return (
        <>
          {field("url", t("webhooks.url", "Webhook URL"), form.url, (v) => setForm({ ...form, url: v }), true)}
          {field("serverUrl", t("webhooks.serverUrl", "Server URL"), form.serverUrl, (v) => setForm({ ...form, serverUrl: v }))}
          {field("topic", t("webhooks.topic", "Topic"), form.topic, (v) => setForm({ ...form, topic: v }))}
          {field("token", t("webhooks.token", "Token"), form.token, (v) => setForm({ ...form, token: v }), true)}
          {numberField()}
        </>
      );
    }
    if (form.provider === "gotify") {
      return (
        <>
          {field("serverUrl", t("webhooks.serverUrl", "Server URL"), form.serverUrl, (v) => setForm({ ...form, serverUrl: v }))}
          {field("token", t("webhooks.appToken", "App token"), form.token, (v) => setForm({ ...form, token: v }), true)}
          {numberField()}
        </>
      );
    }
    return (
      <>
        {field("url", t("webhooks.url", "Webhook URL"), form.url, (v) => setForm({ ...form, url: v }), true)}
        {(form.provider === "feishu" || form.provider === "dingtalk") &&
          field("secret", t("webhooks.secret", "Signing secret"), form.secret, (v) => setForm({ ...form, secret: v }), true)}
      </>
    );
  }

  function field(id: string, label: string, value: string, onChange: (value: string) => void, secret = false) {
    return (
      <div style={{ marginBottom: "12px" }}>
        <label htmlFor={`webhook-${id}`} style={labelStyle}>{label}</label>
        <input
          id={`webhook-${id}`}
          type={secret ? "password" : "text"}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          style={inputStyle}
          autoComplete="off"
        />
      </div>
    );
  }

  function numberField() {
    return (
      <div style={{ marginBottom: "12px" }}>
        <label htmlFor="webhook-priority" style={labelStyle}>{t("webhooks.priority", "Priority")}</label>
        <input
          id="webhook-priority"
          type="number"
          value={form.priority}
          onChange={(e) => setForm({ ...form, priority: Number(e.target.value) })}
          style={inputStyle}
        />
      </div>
    );
  }

  return (
    <div>
      <div
        style={{
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: "16px",
          marginBottom: "20px",
        }}
      >
        <div style={{ minWidth: 0 }}>
          <h2 style={{ margin: 0, fontSize: "16px", fontWeight: 600, color: "var(--color-text-primary)" }}>
            {t("webhooks.title", "Webhook Notifications")}
          </h2>
          <p style={{ margin: "6px 0 0", fontSize: "12px", lineHeight: 1.45, color: "var(--color-text-secondary)" }}>
            {t("webhooks.description", "Send new mail and rule notifications to webhook services. URLs and tokens are stored encrypted and are not shown after saving.")}
          </p>
        </div>
        <button onClick={startCreate} style={primaryButtonStyle}>
          <Plus size={14} />
          {t("webhooks.add", "Add webhook")}
        </button>
      </div>

      {editing && (
        <div
          style={{
            padding: "16px",
            border: "1px solid var(--color-border)",
            borderRadius: "8px",
            backgroundColor: "var(--color-bg)",
            marginBottom: "16px",
            display: "flex",
            flexDirection: "column",
            gap: "2px",
          }}
        >
          {field("name", t("webhooks.name", "Name"), form.name, (v) => setForm({ ...form, name: v }))}
          <div style={{ marginBottom: "12px" }}>
            <label htmlFor="webhook-provider" style={labelStyle}>{t("webhooks.provider", "Provider")}</label>
            <select id="webhook-provider" value={form.provider} onChange={(e) => setForm({ ...form, provider: e.target.value as WebhookProvider })} style={inputStyle}>
              {PROVIDERS.map((provider) => <option key={provider} value={provider}>{t(`webhooks.provider_${provider}`, provider)}</option>)}
            </select>
          </div>
          {renderProviderFields()}
          <label style={{ display: "flex", gap: "8px", alignItems: "center", fontSize: "13px", marginBottom: "8px" }}>
            <input type="checkbox" checked={form.isEnabled} onChange={(e) => setForm({ ...form, isEnabled: e.target.checked })} />
            {t("webhooks.enabled", "Enabled")}
          </label>
          <label style={{ display: "flex", gap: "8px", alignItems: "center", fontSize: "13px", marginBottom: "12px" }}>
            <input type="checkbox" checked={form.notifyOnNewMail} onChange={(e) => setForm({ ...form, notifyOnNewMail: e.target.checked })} />
            {t("webhooks.notifyOnNewMail", "Notify on new mail")}
          </label>
          {error && <p style={{ color: "#ef4444", fontSize: "12px" }}>{error}</p>}
          <div style={{ display: "flex", gap: "8px", justifyContent: "flex-end", marginTop: "4px" }}>
            <button onClick={() => void runTest()} style={secondaryButtonStyle}>{t("webhooks.test", "Test")}</button>
            <button onClick={() => { setEditing(false); setForm(emptyForm); }} style={secondaryButtonStyle}>{t("common.cancel")}</button>
            <button
              onClick={save}
              disabled={saving}
              style={{
                ...primaryButtonStyle,
                opacity: saving ? 0.65 : 1,
                cursor: saving ? "not-allowed" : "pointer",
              }}
            >
              {t("common.save")}
            </button>
          </div>
        </div>
      )}

      <div style={{ border: "1px solid var(--color-border)", borderRadius: "8px", overflow: "hidden", backgroundColor: "var(--color-bg)" }}>
        {endpoints.length === 0 ? (
          <p style={{ margin: 0, padding: "32px 20px", color: "var(--color-text-secondary)", fontSize: "13px", textAlign: "center" }}>{t("webhooks.empty", "No webhook endpoints configured.")}</p>
        ) : endpoints.map((endpoint, index) => (
          <div
            key={endpoint.id}
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: "12px",
              padding: "12px",
              borderBottom: index === endpoints.length - 1 ? "none" : "1px solid var(--color-border)",
            }}
          >
            <button
              onClick={() => startEdit(endpoint)}
              style={{
                flex: 1,
                minWidth: 0,
                textAlign: "left",
                background: "transparent",
                border: "none",
                color: "var(--color-text-primary)",
                cursor: "pointer",
                padding: 0,
              }}
            >
              <strong style={{ display: "block", fontSize: "13px", fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{endpoint.name}</strong>
              <span style={{ display: "block", marginTop: "4px", fontSize: "12px", color: "var(--color-text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {t(`webhooks.provider_${endpoint.provider}`, endpoint.provider)} · {endpoint.is_enabled ? t("webhooks.enabled", "Enabled") : t("rules.disabled", "Disabled")}
                {endpoint.notify_on_new_mail ? ` · ${t("webhooks.notifyOnNewMail", "Notify on new mail")}` : ""}
              </span>
            </button>
            <button onClick={() => runTest(endpoint)} style={secondaryButtonStyle}>{t("webhooks.test", "Test")}</button>
            <button onClick={() => remove(endpoint)} title={t("common.delete", "Delete")} style={iconButtonStyle}><Trash2 size={14} /></button>
          </div>
        ))}
      </div>
    </div>
  );
}
