import { useEffect, useRef, useCallback } from "react";
import { useComposeStore } from "@/stores/compose.store";
import { saveDraft } from "@/lib/api";
import { hasComposeDraft, type ComposeAttachment } from "@/features/compose/compose-draft";

import type { EditorMode } from "./useComposeEditor";

const DRAFT_STORAGE_KEY = "pebble-compose-draft";
const MAX_AUTOSAVE_RETRIES = 3;
const AUTOSAVE_RETRY_BASE_DELAY_MS = 1000;

export interface DraftData {
  accountId: string;
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  rawSource: string;
  richTextHtml: string;
  editorMode: EditorMode;
  attachments: ComposeAttachment[];
  savedAt: number;
}

// Web never restores plaintext browser draft bodies.
export function loadDraftFromStorage(_validAccountIds?: string[]): DraftData | null {
  return null;
}

export function clearDraftStorage() {
  localStorage.removeItem(DRAFT_STORAGE_KEY);
}

interface UseComposeDraftArgs {
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  rawSource: string;
  richTextHtml: string;
  editorMode: EditorMode;
  composeMode: string | null;
  fromAccountId: string | null;
  attachments: ComposeAttachment[];
  /** The current compose was restored from legacy WebView storage. */
  migrateLegacyDraft?: boolean;
  /** True once the TipTap editor has mounted and populated richTextHtml with
   * its initial content (signature, quoted reply, etc.). Until this flips to
   * true, the snapshot would compare user edits against an empty string and
   * falsely report the compose as dirty. */
  editorReady: boolean;
}

export function useComposeDraft({
  to, cc, bcc, subject, rawSource, richTextHtml, editorMode, composeMode, fromAccountId, attachments, editorReady,
  migrateLegacyDraft = false,
}: UseComposeDraftArgs) {
  // Snapshot the initial compose state so pre-populated reply/forward
  // fields don't immediately trigger the "unsaved draft" guard.
  // Deferred until the editor has rendered its initial content - taken once,
  // in an effect that runs after the first render post-editorReady.
  const initialSnapshot = useRef<{
    to: string[]; cc: string[]; bcc: string[]; subject: string;
    rawSource: string; richTextHtml: string; attachments: ComposeAttachment[];
  } | null>(null);
  useEffect(() => {
    if (!editorReady || initialSnapshot.current) return;
    initialSnapshot.current = {
      to: [...to], cc: [...cc], bcc: [...bcc], subject,
      rawSource, richTextHtml, attachments: attachments.map((a) => ({ ...a })),
    };
    // Only depend on editorReady - we want this to run once after mount, not
    // each time the user edits.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editorReady]);

  const arraysEqual = useCallback(
    (a: string[], b: string[]) => a.length === b.length && a.every((v, i) => v === b[i]),
    [],
  );
  const attachmentsEqual = useCallback(
    (a: ComposeAttachment[], b: ComposeAttachment[]) =>
      a.length === b.length &&
      a.every((v, i) => v.name === b[i]?.name && v.path === b[i]?.path && v.size === b[i]?.size),
    [],
  );

  // Ref to track the server-side draft ID across saves.
  // Scoped per account: when the user switches From, the prior draft_id
  // belongs to a different account and must not be reused.
  const draftIdRef = useRef<string | null>(null);
  const draftAccountRef = useRef<string | null>(null);
  const draftIdsByAccountRef = useRef<Record<string, string>>({});
  const queuedSaveByAccountRef = useRef<Record<string, Parameters<typeof saveDraft>[0] | undefined>>({});
  const saveWorkerByAccountRef = useRef<Record<string, Promise<void> | undefined>>({});
  const cancelRetryDelayByAccountRef = useRef<Record<string, (() => void) | undefined>>({});
  const persistenceCancelledRef = useRef(false);
  const legacyMigrationPendingRef = useRef(migrateLegacyDraft);
  if (draftAccountRef.current !== fromAccountId) {
    draftAccountRef.current = fromAccountId;
    draftIdRef.current = fromAccountId ? draftIdsByAccountRef.current[fromAccountId] ?? null : null;
  }

  function waitForRetryDelay(accountId: string, delayMs: number): Promise<boolean> {
    return new Promise((resolve) => {
      let settled = false;
      const finish = (shouldRetry: boolean) => {
        if (settled) return;
        settled = true;
        delete cancelRetryDelayByAccountRef.current[accountId];
        resolve(shouldRetry);
      };
      const timer = setTimeout(() => finish(true), delayMs);
      cancelRetryDelayByAccountRef.current[accountId] = () => {
        clearTimeout(timer);
        finish(false);
      };
    });
  }

  function startSaveWorker(accountId: string) {
    if (persistenceCancelledRef.current) return;
    if (saveWorkerByAccountRef.current[accountId]) return;
    const worker = (async () => {
      let consecutiveFailures = 0;
      try {
        let queued = queuedSaveByAccountRef.current[accountId];
        while (queued && !persistenceCancelledRef.current) {
          delete queuedSaveByAccountRef.current[accountId];
          try {
            const id = await saveDraft({
              ...queued,
              existingDraftId: draftIdsByAccountRef.current[accountId] || undefined,
            });
            draftIdsByAccountRef.current[accountId] = id;
            if (draftAccountRef.current === accountId) {
              draftIdRef.current = id;
            }
            if (legacyMigrationPendingRef.current) {
              clearDraftStorage();
              legacyMigrationPendingRef.current = false;
            }
            consecutiveFailures = 0;
          } catch (err) {
            console.warn("Backend draft save failed:", err);
            consecutiveFailures += 1;
            const newerQueued = queuedSaveByAccountRef.current[accountId];
            if (newerQueued) {
              queued = newerQueued;
              consecutiveFailures = 0;
              continue;
            }
            if (persistenceCancelledRef.current
              || consecutiveFailures > MAX_AUTOSAVE_RETRIES) {
              break;
            }
            queuedSaveByAccountRef.current[accountId] = queued;
            const shouldRetry = await waitForRetryDelay(
              accountId,
              AUTOSAVE_RETRY_BASE_DELAY_MS * 2 ** (consecutiveFailures - 1),
            );
            if (!shouldRetry || persistenceCancelledRef.current) break;
          }
          const nextQueued = queuedSaveByAccountRef.current[accountId];
          if (nextQueued && nextQueued !== queued) {
            consecutiveFailures = 0;
          }
          queued = nextQueued;
        }
      } finally {
        delete saveWorkerByAccountRef.current[accountId];
        if (queuedSaveByAccountRef.current[accountId]) {
          startSaveWorker(accountId);
        }
      }
    })();
    saveWorkerByAccountRef.current[accountId] = worker;
  }

  function queueDraftSave(args: Parameters<typeof saveDraft>[0]) {
    if (persistenceCancelledRef.current) return;
    queuedSaveByAccountRef.current[args.accountId] = args;
    startSaveWorker(args.accountId);
  }

  const cancelPendingDraftSaves = useCallback(async () => {
    persistenceCancelledRef.current = true;
    queuedSaveByAccountRef.current = {};
    for (const cancelRetryDelay of Object.values(cancelRetryDelayByAccountRef.current)) {
      cancelRetryDelay?.();
    }
    const activeWorkers = Object.values(saveWorkerByAccountRef.current).filter(
      (worker): worker is Promise<void> => Boolean(worker),
    );
    await Promise.allSettled(activeWorkers);
    const savedDraftIds = { ...draftIdsByAccountRef.current };
    draftIdsByAccountRef.current = {};
    draftIdRef.current = null;
    return savedDraftIds;
  }, []);

  useEffect(() => {
    persistenceCancelledRef.current = false;
    return () => {
      persistenceCancelledRef.current = true;
      queuedSaveByAccountRef.current = {};
      for (const cancelRetryDelay of Object.values(cancelRetryDelayByAccountRef.current)) {
        cancelRetryDelay?.();
      }
    };
  }, []);

  // Track dirty state for leave-protection.
  // Skip until the initial snapshot is captured (i.e. editor ready).
  useEffect(() => {
    const init = initialSnapshot.current;
    if (!init) return;
    const userChanged =
      !arraysEqual(to, init.to) ||
      !arraysEqual(cc, init.cc) ||
      !arraysEqual(bcc, init.bcc) ||
      subject !== init.subject ||
      rawSource !== init.rawSource ||
      richTextHtml !== init.richTextHtml ||
      !attachmentsEqual(attachments, init.attachments);
    useComposeStore.getState().setComposeDirty(userChanged);
  }, [arraysEqual, attachments, attachmentsEqual, bcc, cc, rawSource, richTextHtml, subject, to, editorReady]);

  // Auto-save draft to backend (debounced 3s). Do not persist plaintext draft
  // data in WebView storage.
  useEffect(() => {
    if (!composeMode || !editorReady || !initialSnapshot.current) return;
    const timer = setTimeout(() => {
      const draftAttachments = attachments.filter((attachment) =>
        attachment.path.trim().length > 0 || attachment.name.trim().length > 0,
      );
      const hasDraft = hasComposeDraft({
        to, cc, bcc, subject, rawSource, richTextHtml, attachments: draftAttachments,
      });
      if (hasDraft && fromAccountId) {
        const accountIdAtSave = fromAccountId;
        // Save to backend under the current From account.
        {
          // Pick body source based on current editor mode to avoid stale content.
          // For rich text, strip HTML tags to produce a plain-text fallback.
          const bodyText = editorMode === "rich"
            ? richTextHtml.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim()
            : rawSource;
          const bodyHtml = editorMode === "rich" ? richTextHtml : (editorMode === "html" ? rawSource : undefined);
          queueDraftSave({
            accountId: accountIdAtSave,
            to, cc, bcc, subject,
            bodyText,
            bodyHtml: bodyHtml || undefined,
            attachmentPaths: draftAttachments.map((attachment) => attachment.path).filter(Boolean),
          });
        }
      }
    }, 3000);
    return () => clearTimeout(timer);
  }, [attachments, to, cc, bcc, subject, rawSource, richTextHtml, editorMode, composeMode, fromAccountId, editorReady]);

  return { draftIdRef, draftIdsByAccountRef, cancelPendingDraftSaves };
}
