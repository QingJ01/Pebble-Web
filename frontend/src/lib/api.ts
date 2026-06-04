import api from "../api-client";

// Re-export all IPC types so existing `import { Foo } from "@/lib/api"` keeps working.
export type {
  Account,
  AccountProxyMode,
  AccountProxySetting,
  AddAccountRequest,
  AdvancedSearchQuery,
  AppLogSnapshot,
  Attachment,
  BackupPreview,
  ConnectionSecurity,
  EmailAddress,
  Folder,
  HttpProxyConfig,
  ImportedBackgroundImage,
  KanbanCard,
  KanbanColumnType,
  KnownContact,
  Label,
  Message,
  MessageSummary,
  NotificationStatus,
  PendingMailOp,
  PendingMailOpsSummary,
  PrivacyMode,
  RenderedHtml,
  Rule,
  SearchHit,
  SnoozedMessage,
  ThreadSummary,
  TranslateConfig,
  TranslateResult,
  TrustedSender,
  WebhookConfig,
  WebhookEndpoint,
  WebhookProvider,
} from "./ipc-types";

import type {
  Account,
  AccountProxyMode,
  AccountProxySetting,
  AddAccountRequest,
  AdvancedSearchQuery,
  AppLogSnapshot,
  Attachment,
  BackupPreview,
  ConnectionSecurity,
  Folder,
  HttpProxyConfig,
  KanbanCard,
  KanbanColumnType,
  KnownContact,
  Label,
  Message,
  MessageSummary,
  NotificationStatus,
  PendingMailOp,
  PendingMailOpsSummary,
  PrivacyMode,
  RenderedHtml,
  Rule,
  SearchHit,
  SnoozedMessage,
  ThreadSummary,
  TranslateConfig,
  TranslateResult,
  TrustedSender,
  WebhookConfig,
  WebhookEndpoint,
  WebhookProvider,
} from "./ipc-types";

// ─── Account API ─────────────────────────────────────────────────────────────

export async function healthCheck(): Promise<string> {
  const res = await api.get<string>("/health");
  return res.data;
}

export async function readAppLog(_maxBytes: number): Promise<AppLogSnapshot> {
  // Not implemented in web backend
  return { path: "", content: "", truncated: false };
}

export async function getGlobalProxy(): Promise<HttpProxyConfig | null> {
  console.warn("[api] getGlobalProxy not implemented in web");
  return null;
}

export async function getAccountProxy(_accountId: string): Promise<HttpProxyConfig | null> {
  console.warn("[api] getAccountProxy not implemented in web");
  return null;
}

export async function getAccountProxySetting(_accountId: string): Promise<AccountProxySetting> {
  console.warn("[api] getAccountProxySetting not implemented in web");
  return { mode: "global" as AccountProxyMode, proxy: null };
}

export async function updateAccountProxy(
  _accountId: string,
  _proxyHost?: string,
  _proxyPort?: number,
): Promise<void> {
  console.warn("[api] updateAccountProxy not implemented in web");
}

export async function updateAccountProxySetting(
  _accountId: string,
  _mode: AccountProxyMode,
  _proxyHost?: string,
  _proxyPort?: number,
): Promise<void> {
  console.warn("[api] updateAccountProxySetting not implemented in web");
}

export async function updateGlobalProxy(
  _proxyHost?: string,
  _proxyPort?: number,
): Promise<void> {
  console.warn("[api] updateGlobalProxy not implemented in web");
}

export async function completeOAuthFlow(
  _provider: string,
  _email: string,
  _displayName: string,
  _proxyHost?: string,
  _proxyPort?: number,
): Promise<Account> {
  throw new Error("OAuth flow is not supported in the web version");
}

export async function getOAuthAccountProxy(_accountId: string): Promise<HttpProxyConfig | null> {
  console.warn("[api] getOAuthAccountProxy not implemented in web");
  return null;
}

export async function getOAuthAccountProxySetting(_accountId: string): Promise<AccountProxySetting> {
  console.warn("[api] getOAuthAccountProxySetting not implemented in web");
  return { mode: "global" as AccountProxyMode, proxy: null };
}

export async function updateOAuthAccountProxy(
  _accountId: string,
  _proxyHost?: string,
  _proxyPort?: number,
): Promise<void> {
  console.warn("[api] updateOAuthAccountProxy not implemented in web");
}

export async function updateOAuthAccountProxySetting(
  _accountId: string,
  _mode: AccountProxyMode,
  _proxyHost?: string,
  _proxyPort?: number,
): Promise<void> {
  console.warn("[api] updateOAuthAccountProxySetting not implemented in web");
}

export async function addAccount(request: AddAccountRequest): Promise<Account> {
  const res = await api.post<Account>("/accounts", request);
  return res.data;
}

export async function testAccountConnection(accountId: string): Promise<string> {
  const res = await api.post<{ ok: boolean; report: string }>(`/accounts/${accountId}/test-connection`);
  return res.data.report;
}

export async function testImapConnection(
  imapHost: string,
  imapPort: number,
  imapSecurity: ConnectionSecurity,
  _proxyHost?: string,
  _proxyPort?: number,
  username?: string,
  password?: string,
): Promise<string> {
  const res = await api.post<{ ok: boolean; report: string }>("/test-imap-connection", {
    imapHost, imapPort, imapSecurity, username, password,
  });
  return res.data.report;
}

export async function listAccounts(): Promise<Account[]> {
  const res = await api.get<Account[]>("/accounts");
  return res.data;
}

export async function updateAccount(
  accountId: string,
  email: string,
  displayName: string,
  password?: string,
  imapHost?: string,
  imapPort?: number,
  smtpHost?: string,
  smtpPort?: number,
  imapSecurity?: ConnectionSecurity,
  smtpSecurity?: ConnectionSecurity,
  _proxyHost?: string,
  _proxyPort?: number,
  accountColor?: string,
): Promise<void> {
  await api.put(`/accounts/${accountId}`, {
    email,
    displayName,
    color: accountColor,
    password: password || undefined,
    imapHost,
    imapPort,
    smtpHost,
    smtpPort,
    imapSecurity,
    smtpSecurity,
  });
}

export async function deleteAccount(accountId: string): Promise<void> {
  await api.delete(`/accounts/${accountId}`);
}

// ─── Folder API ──────────────────────────────────────────────────────────────

export async function listFolders(accountId: string): Promise<Folder[]> {
  const res = await api.get<Folder[]>(`/accounts/${accountId}/folders`);
  return res.data;
}

// ─── Message API ─────────────────────────────────────────────────────────────

export async function listMessages(
  folderId: string,
  limit: number,
  offset: number,
  _folderIds?: string[],
): Promise<MessageSummary[]> {
  const res = await api.get<MessageSummary[]>(`/folders/${folderId}/messages`, {
    params: { limit, offset },
  });
  return res.data;
}

export async function listStarredMessages(
  accountId: string,
  limit: number,
  offset: number,
): Promise<MessageSummary[]> {
  const res = await api.get<MessageSummary[]>(`/accounts/${accountId}/starred`, {
    params: { limit, offset },
  });
  return res.data;
}

export async function getMessage(messageId: string): Promise<Message | null> {
  const res = await api.get<Message | null>(`/messages/${messageId}`);
  return res.data;
}

export async function getMessagesBatch(messageIds: string[]): Promise<Message[]> {
  const res = await api.post<Message[]>("/messages/batch", { messageIds });
  return res.data;
}

export async function getRenderedHtml(
  messageId: string,
  privacyMode: PrivacyMode,
): Promise<RenderedHtml> {
  const res = await api.post<RenderedHtml>(`/messages/${messageId}/render`, { privacyMode });
  return res.data;
}

export async function getMessageWithHtml(
  messageId: string,
  privacyMode: PrivacyMode,
): Promise<[Message, RenderedHtml] | null> {
  const res = await api.post<[Message, RenderedHtml] | null>(`/messages/${messageId}/with-html`, { privacyMode });
  return res.data;
}

export async function updateMessageFlags(
  messageId: string,
  isRead?: boolean,
  isStarred?: boolean,
): Promise<void> {
  await api.put(`/messages/${messageId}/flags`, { isRead, isStarred });
}

const archivingIds = new Set<string>();

export async function archiveMessage(messageId: string): Promise<string> {
  if (archivingIds.has(messageId)) {
    return "skipped";
  }
  archivingIds.add(messageId);
  try {
    const res = await api.post<string>(`/messages/${messageId}/archive`);
    return res.data;
  } finally {
    archivingIds.delete(messageId);
  }
}

export async function deleteMessage(messageId: string): Promise<void> {
  await api.delete(`/messages/${messageId}`);
}

export async function restoreMessage(messageId: string): Promise<void> {
  await api.post(`/messages/${messageId}/restore`);
}

export async function moveToFolder(messageId: string, targetFolderId: string): Promise<void> {
  await api.post(`/messages/${messageId}/move`, { folderId: targetFolderId });
}

export async function emptyTrash(accountId: string): Promise<number> {
  const res = await api.post<number>(`/accounts/${accountId}/empty-trash`);
  return res.data;
}

export async function getPendingMailOpsSummary(
  accountId: string | null,
): Promise<PendingMailOpsSummary> {
  const res = await api.get<PendingMailOpsSummary>("/pending-ops/summary", {
    params: { accountId },
  });
  return res.data;
}

export async function listPendingMailOps(
  accountId: string | null,
  limit = 100,
): Promise<PendingMailOp[]> {
  const res = await api.get<PendingMailOp[]>("/pending-ops", {
    params: { accountId, limit },
  });
  return res.data;
}

// ─── Trusted Senders API ────────────────────────────────────────────────────

export async function listTrustedSenders(accountId: string): Promise<TrustedSender[]> {
  const res = await api.get<TrustedSender[]>(`/accounts/${accountId}/trusted-senders`);
  return res.data;
}

export async function removeTrustedSender(accountId: string, email: string): Promise<void> {
  await api.delete(`/accounts/${accountId}/trusted-senders/${encodeURIComponent(email)}`);
}

export async function trustSender(accountId: string, email: string, trustType: "images" | "all"): Promise<void> {
  await api.post(`/accounts/${accountId}/trusted-senders`, { email, trustType });
}

export async function isTrustedSender(accountId: string, email: string): Promise<boolean> {
  const normalized = email.trim().toLowerCase();
  const senders = await listTrustedSenders(accountId);
  return senders.some((sender) => sender.email.toLowerCase() === normalized);
}

// ─── Search API ──────────────────────────────────────────────────────────────

export async function searchMessages(
  query: string,
  limit?: number,
): Promise<SearchHit[]> {
  const res = await api.post<SearchHit[]>("/search", { query, limit });
  return res.data;
}

export async function advancedSearch(
  query: AdvancedSearchQuery,
  limit?: number,
): Promise<SearchHit[]> {
  const res = await api.post<SearchHit[]>("/search", {
    query: query.text,
    from: query.from,
    to: query.to,
    subject: query.subject,
    dateFrom: query.dateFrom,
    dateTo: query.dateTo,
    hasAttachment: query.hasAttachment,
    folderId: query.folderId,
    limit,
  });
  return res.data;
}

// ─── Sync API ────────────────────────────────────────────────────────────────

export async function startSync(accountId: string, _pollIntervalSecs?: number): Promise<string> {
  await api.post("/sync/trigger", { accountId });
  return "ok";
}

export async function triggerSync(accountId: string, reason: string): Promise<void> {
  await api.post("/sync/trigger", { accountId, reason });
}

export type RealtimePreference = "realtime" | "balanced" | "battery" | "manual";

export async function setRealtimePreference(_mode: RealtimePreference): Promise<void> {
  // No-op in web — realtime preference is a desktop feature
}

export async function setNotificationsEnabled(_enabled: boolean): Promise<void> {
  // No-op in web — desktop notifications not supported
}

export async function getNotificationStatus(): Promise<NotificationStatus> {
  return { enabled: false, attention_active: false, platform: "web", app_id: null };
}

export async function showTestNotification(): Promise<void> {
  // No-op in web
}

export async function clearNotificationAttention(): Promise<void> {
  // No-op in web
}

export async function setTrayMenuLabels(_showLabel: string, _hideLabel: string, _quitLabel: string): Promise<void> {
  // No-op in web — no system tray
}

export async function stopSync(_accountId: string): Promise<void> {
  // No-op in web
}

// ─── Attachment API ──────────────────────────────────────────────────────────

export async function listAttachments(messageId: string): Promise<Attachment[]> {
  const res = await api.get<Attachment[]>(`/messages/${messageId}/attachments`);
  return res.data;
}

export async function getAttachmentPath(_attachmentId: string): Promise<string | null> {
  return null;
}

export async function downloadAttachment(attachmentId: string, saveTo?: string): Promise<string> {
  const res = await api.get(`/attachments/${attachmentId}/download`, { responseType: "blob" });
  const blob = new Blob([res.data]);
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = saveTo || attachmentId;
  document.body.appendChild(a);
  a.click();
  a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
  return saveTo || attachmentId;
}

// ─── Kanban API ──────────────────────────────────────────────────────────────

export async function moveToKanban(messageId: string, column: KanbanColumnType, position?: number): Promise<void> {
  await api.post(`/kanban/${messageId}`, { column, position });
}

export async function listKanbanCards(column?: KanbanColumnType): Promise<KanbanCard[]> {
  const res = await api.get<KanbanCard[]>("/kanban", { params: { column } });
  return res.data;
}

export async function removeFromKanban(messageId: string): Promise<void> {
  await api.delete(`/kanban/${messageId}`);
}

export async function listKanbanContextNotes(): Promise<Record<string, string>> {
  const res = await api.get<Record<string, string>>("/kanban/context-notes");
  return res.data;
}

export async function setKanbanContextNote(
  messageId: string,
  note: string,
): Promise<Record<string, string>> {
  const res = await api.put<Record<string, string>>(`/kanban/context-notes/${messageId}`, { note });
  return res.data;
}

export async function mergeKanbanContextNotes(
  notes: Record<string, string>,
): Promise<Record<string, string>> {
  const res = await api.post<Record<string, string>>("/kanban/context-notes", { notes });
  return res.data;
}

// ─── Snooze API ──────────────────────────────────────────────────────────────

export async function snoozeMessage(messageId: string, until: number, returnTo: string): Promise<void> {
  await api.post(`/messages/${messageId}/snooze`, { until, returnTo });
}

export async function unsnoozeMessage(messageId: string): Promise<void> {
  await api.delete(`/messages/${messageId}/snooze`);
}

export async function listSnoozed(): Promise<SnoozedMessage[]> {
  const res = await api.get<SnoozedMessage[]>("/snoozed");
  return res.data;
}

// ─── Rules API ───────────────────────────────────────────────────────────────

export async function createRule(name: string, priority: number, conditions: string, actions: string): Promise<Rule> {
  const res = await api.post<Rule>("/rules", {
    name,
    priority,
    conditions,
    actions,
    is_enabled: true,
  });
  return res.data;
}

export async function listRules(): Promise<Rule[]> {
  const res = await api.get<Rule[]>("/rules");
  return res.data;
}

export async function updateRule(rule: Rule): Promise<void> {
  await api.put(`/rules/${rule.id}`, {
    name: rule.name,
    priority: rule.priority,
    conditions: rule.conditions,
    actions: rule.actions,
    is_enabled: rule.is_enabled,
  });
}

export async function deleteRule(ruleId: string): Promise<void> {
  await api.delete(`/rules/${ruleId}`);
}

// ─── Webhook API ─────────────────────────────────────────────────────────────

export async function listWebhooks(): Promise<WebhookEndpoint[]> {
  const res = await api.get<WebhookEndpoint[]>("/webhooks");
  return res.data;
}

export async function createWebhook(
  name: string,
  provider: WebhookProvider,
  config: WebhookConfig,
  isEnabled: boolean,
  notifyOnNewMail: boolean,
): Promise<WebhookEndpoint> {
  const res = await api.post<WebhookEndpoint>("/webhooks", {
    name,
    provider,
    config,
    isEnabled,
    notifyOnNewMail,
  });
  return res.data;
}

export async function updateWebhook(
  endpoint: WebhookEndpoint,
  config: WebhookConfig,
): Promise<void> {
  await api.put(`/webhooks/${endpoint.id}`, {
    name: endpoint.name,
    provider: endpoint.provider,
    config,
    isEnabled: endpoint.is_enabled,
    notifyOnNewMail: endpoint.notify_on_new_mail,
  });
}

export async function deleteWebhook(endpointId: string): Promise<void> {
  await api.delete(`/webhooks/${endpointId}`);
}

export async function testWebhook(provider: WebhookProvider, config: WebhookConfig): Promise<void> {
  await api.post("/webhooks/test", { provider, config });
}

export async function testSavedWebhook(endpointId: string): Promise<void> {
  await api.post(`/webhooks/${endpointId}/test`);
}

// ─── Compose API ─────────────────────────────────────────────────────────────

export async function sendEmail(
  accountId: string,
  to: string[],
  cc: string[],
  bcc: string[],
  subject: string,
  bodyText: string,
  bodyHtml?: string,
  inReplyTo?: string,
  attachmentPaths?: string[],
): Promise<void> {
  await api.post("/compose", {
    accountId, to, cc, bcc, subject, bodyText, bodyHtml, inReplyTo, attachmentPaths,
  });
}

export async function stageComposeAttachment(filename: string, bytes: number[]): Promise<string> {
  const uint8 = new Uint8Array(bytes);
  // Efficient base64 conversion
  const chunks: string[] = [];
  const chunkSize = 0x8000;
  for (let i = 0; i < uint8.length; i += chunkSize) {
    chunks.push(String.fromCharCode.apply(null, uint8.subarray(i, i + chunkSize) as unknown as number[]));
  }
  const data = btoa(chunks.join(''));
  const res = await api.post<{ path: string }>("/compose/attachment", { filename, data });
  return res.data.path;
}

// ─── Batch Operations ───────────────────────────────────────────────────────

export async function batchArchive(messageIds: string[]): Promise<number> {
  const res = await api.post<number>("/messages/batch/archive", { messageIds });
  return res.data;
}

export async function batchDelete(messageIds: string[]): Promise<number> {
  const res = await api.post<number>("/messages/batch/delete", { messageIds });
  return res.data;
}

export async function batchMarkRead(messageIds: string[], isRead: boolean): Promise<number> {
  const res = await api.post<number>("/messages/batch/mark-read", { messageIds, isRead });
  return res.data;
}

export async function batchStar(messageIds: string[], starred: boolean): Promise<number> {
  const res = await api.post<number>("/messages/batch/star", { messageIds, starred });
  return res.data;
}

// ─── Translate API ───────────────────────────────────────────────────────────

export async function translateText(text: string, fromLang: string, toLang: string): Promise<TranslateResult> {
  const res = await api.post<TranslateResult>("/translate", { text, source_lang: fromLang, target_lang: toLang });
  return res.data;
}

export async function getTranslateConfig(): Promise<TranslateConfig | null> {
  const res = await api.get<{ providerType: string; config: string; isEnabled: boolean } | null>("/translate/config");
  if (!res.data) return null;
  return {
    id: "active",
    provider_type: res.data.providerType,
    config: res.data.config,
    is_enabled: res.data.isEnabled,
    created_at: 0,
    updated_at: 0,
  };
}

export async function saveTranslateConfig(providerType: string, config: string, isEnabled: boolean): Promise<void> {
  await api.post("/translate/config", { providerType, config, isEnabled });
}

export async function testTranslateConnection(config: string): Promise<string> {
  const res = await api.post<{ ok: boolean; result: string }>("/translate/test", { providerType: "test", config, isEnabled: true });
  return res.data.result;
}

// ─── Thread API ──────────────────────────────────────────────────────────────

export async function listThreads(
  folderId: string,
  limit: number,
  offset: number,
  _folderIds?: string[],
): Promise<ThreadSummary[]> {
  const res = await api.get<ThreadSummary[]>(`/folders/${folderId}/threads`, {
    params: { limit, offset },
  });
  return res.data;
}

export async function listThreadMessages(threadId: string): Promise<Message[]> {
  const res = await api.get<Message[]>(`/threads/${threadId}/messages`);
  return res.data;
}

// ─── Labels API ──────────────────────────────────────────────────────────────

export async function getMessageLabels(messageId: string): Promise<Label[]> {
  const res = await api.get<Label[]>(`/messages/${messageId}/labels`);
  return res.data;
}

export async function getMessageLabelsBatch(messageIds: string[]): Promise<Record<string, Label[]>> {
  const res = await api.post<Record<string, Label[]>>("/messages/labels/batch", { messageIds });
  return res.data;
}

export async function addMessageLabel(messageId: string, labelName: string): Promise<void> {
  await api.post(`/messages/${messageId}/labels`, { label_name: labelName });
}

export async function removeMessageLabel(messageId: string, labelName: string): Promise<void> {
  await api.delete(`/messages/${messageId}/labels/${encodeURIComponent(labelName)}`);
}

export async function listLabels(): Promise<Label[]> {
  const res = await api.get<Label[]>("/labels");
  return res.data;
}

export async function createLabel(name: string, color?: string): Promise<Label> {
  const res = await api.post<Label>("/labels", { name, color });
  return res.data;
}

export async function deleteLabel(id: string): Promise<void> {
  await api.delete(`/labels/${id}`);
}

// ─── Cloud Sync API ─────────────────────────────────────────────────────────

export async function testWebdavConnection(url: string, username: string, password: string): Promise<string> {
  const res = await api.post<{ result: string }>("/cloud-sync/test", { url, username, password });
  return res.data.result;
}

export async function backupToWebdav(url: string, username: string, password: string): Promise<string> {
  const res = await api.post<{ result: string }>("/cloud-sync/backup", { url, username, password });
  return res.data.result;
}

export async function previewWebdavBackup(url: string, username: string, password: string): Promise<BackupPreview> {
  const res = await api.post<BackupPreview>("/cloud-sync/preview", { url, username, password });
  return res.data;
}

export async function restoreFromWebdav(url: string, username: string, password: string): Promise<string> {
  const res = await api.post<{ result: string }>("/cloud-sync/restore", { url, username, password });
  return res.data.result;
}

// ─── Contacts API ────────────────────────────────────────────────────────────

export async function searchContacts(
  accountId: string,
  query: string,
  limit?: number,
): Promise<KnownContact[]> {
  const res = await api.get<KnownContact[]>("/contacts", {
    params: { accountId, query, limit },
  });
  return res.data;
}

// ─── Drafts API ──────────────────────────────────────────────────────────────

export async function saveDraft(args: {
  accountId: string;
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  bodyText: string;
  bodyHtml?: string;
  inReplyTo?: string;
  existingDraftId?: string;
  attachmentPaths?: string[];
}): Promise<string> {
  const res = await api.post<{ id: string }>("/drafts", args);
  return res.data.id;
}

export async function deleteDraft(accountId: string, draftId: string): Promise<void> {
  await api.delete(`/accounts/${accountId}/drafts/${draftId}`);
}

// ─── Folder Counts API ───────────────────────────────────────────────────────

export async function getFolderUnreadCounts(accountId: string): Promise<Record<string, number>> {
  const res = await api.get<Record<string, number>>(`/accounts/${accountId}/folder-unread-counts`);
  return res.data;
}
