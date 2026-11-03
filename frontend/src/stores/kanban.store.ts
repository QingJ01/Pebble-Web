import { create } from "zustand";
import type { KanbanCard, KanbanColumnType } from "@/lib/api";
import {
  listKanbanCards,
  listKanbanContextNotes,
  mergeKanbanContextNotes,
  moveToKanban,
  removeFromKanban,
  setKanbanContextNote,
} from "@/lib/api";

interface KanbanState {
  cards: KanbanCard[];
  cardIdSet: Set<string>;
  contextNotes: Record<string, string>;
  loading: boolean;
  fetchCards: () => Promise<void>;
  moveCard: (messageId: string, column: KanbanColumnType, position: number) => Promise<void>;
  addCard: (messageId: string, column: KanbanColumnType) => Promise<void>;
  removeCard: (messageId: string) => Promise<void>;
  reorderInColumn: (column: KanbanColumnType, orderedIds: string[]) => void;
  setContextNote: (messageId: string, note: string) => Promise<void>;
}

const LEGACY_CONTEXT_NOTES_STORAGE_KEY = "pebble-kanban-context-notes";

function loadLegacyContextNotes(): Record<string, string> {
  if (typeof localStorage === "undefined") return {};
  try {
    const parsed = JSON.parse(localStorage.getItem(LEGACY_CONTEXT_NOTES_STORAGE_KEY) || "{}");
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : {};
  } catch {
    return {};
  }
}

let legacyContextNotes = loadLegacyContextNotes();
let nextContextNoteGeneration = 0;
const latestContextNoteGeneration = new Map<string, number>();
const contextNoteMutationGeneration = new Map<string, number>();
let nextFetchGeneration = 0;
let latestFetchGeneration = 0;

async function loadContextNotes(): Promise<Record<string, string>> {
  const backendNotes = await listKanbanContextNotes();
  const legacyNotes = legacyContextNotes;
  const legacyEntries = Object.entries(legacyNotes).filter(
    ([messageId, note]) => messageId && note && backendNotes[messageId] === undefined,
  );

  const mergedNotes = legacyEntries.length === 0
    ? backendNotes
    : await mergeKanbanContextNotes(Object.fromEntries(legacyEntries));

  legacyContextNotes = {};
  if (typeof localStorage !== "undefined") {
    try {
      localStorage.removeItem(LEGACY_CONTEXT_NOTES_STORAGE_KEY);
    } catch { /* keep the in-memory migration result */ }
  }
  return mergedNotes;
}

function buildIdSet(cards: KanbanCard[]): Set<string> {
  return new Set(cards.map((c) => c.message_id));
}

export const useKanbanStore = create<KanbanState>((set, get) => ({
  cards: [],
  cardIdSet: new Set<string>(),
  contextNotes: {},
  loading: false,

  fetchCards: async () => {
    const fetchGeneration = ++nextFetchGeneration;
    latestFetchGeneration = fetchGeneration;
    const contextNoteGenerationAtStart = nextContextNoteGeneration;
    const pendingContextNotesAtStart = new Set(latestContextNoteGeneration.keys());
    set({ loading: true });
    try {
      const [cards, contextNotes] = await Promise.all([listKanbanCards(), loadContextNotes()]);
      if (latestFetchGeneration !== fetchGeneration) return;
      set((state) => {
        const mergedContextNotes = { ...contextNotes };
        const notesToPreserve = new Set([
          ...pendingContextNotesAtStart,
          ...latestContextNoteGeneration.keys(),
          ...[...contextNoteMutationGeneration.entries()]
            .filter(([, generation]) => generation > contextNoteGenerationAtStart)
            .map(([messageId]) => messageId),
        ]);
        for (const messageId of notesToPreserve) {
          if (Object.prototype.hasOwnProperty.call(state.contextNotes, messageId)) {
            mergedContextNotes[messageId] = state.contextNotes[messageId];
          } else {
            delete mergedContextNotes[messageId];
          }
        }
        return { cards, cardIdSet: buildIdSet(cards), contextNotes: mergedContextNotes };
      });
    } finally {
      if (latestFetchGeneration === fetchGeneration) set({ loading: false });
    }
  },

  moveCard: async (messageId: string, column: KanbanColumnType, position: number) => {
    // Optimistic update
    const prev = get().cards;
    const updated = prev.map((c) =>
      c.message_id === messageId ? { ...c, column, position } : c,
    );
    set({ cards: updated, cardIdSet: buildIdSet(updated) });
    try {
      await moveToKanban(messageId, column, position);
    } catch {
      // Rollback on error
      set({ cards: prev, cardIdSet: buildIdSet(prev) });
    }
  },

  addCard: async (messageId: string, column: KanbanColumnType) => {
    await moveToKanban(messageId, column);
    await get().fetchCards();
  },

  reorderInColumn: (column, orderedIds) => {
    const prev = get().cards;
    const others = prev.filter((c) => c.column !== column);
    const reordered = orderedIds
      .map((id, i) => {
        const card = prev.find((c) => c.message_id === id);
        return card ? { ...card, position: i } : null;
      })
      .filter((c): c is KanbanCard => c !== null);
    const merged = [...others, ...reordered];
    set({ cards: merged, cardIdSet: buildIdSet(merged) });
    // Persist all position changes and rollback entirely on any failure
    Promise.all(
      reordered.map((card) => moveToKanban(card.message_id, card.column, card.position)),
    ).catch(() => {
      set({ cards: prev, cardIdSet: buildIdSet(prev) });
    });
  },

  removeCard: async (messageId: string) => {
    const prev = get().cards;
    const filtered = prev.filter((c) => c.message_id !== messageId);
    set({ cards: filtered, cardIdSet: buildIdSet(filtered) });
    try {
      await removeFromKanban(messageId);
    } catch {
      set({ cards: prev, cardIdSet: buildIdSet(prev) });
    }
  },

  setContextNote: async (messageId, note) => {
    const prev = get().contextNotes;
    const hadPreviousNote = Object.prototype.hasOwnProperty.call(prev, messageId);
    const previousNote = prev[messageId];
    const generation = ++nextContextNoteGeneration;
    latestContextNoteGeneration.set(messageId, generation);
    contextNoteMutationGeneration.set(messageId, generation);
    const next = { ...prev };
    if (note) {
      next[messageId] = note;
    } else {
      delete next[messageId];
    }
    set({ contextNotes: next });
    try {
      const saved = await setKanbanContextNote(messageId, note);
      if (latestContextNoteGeneration.get(messageId) !== generation) return;
      set((state) => {
        const merged = { ...state.contextNotes };
        if (Object.prototype.hasOwnProperty.call(saved, messageId)) {
          merged[messageId] = saved[messageId];
        } else {
          delete merged[messageId];
        }
        return { contextNotes: merged };
      });
      latestContextNoteGeneration.delete(messageId);
    } catch (err) {
      if (latestContextNoteGeneration.get(messageId) === generation) {
        set((state) => {
          const rolledBack = { ...state.contextNotes };
          if (hadPreviousNote) {
            rolledBack[messageId] = previousNote;
          } else {
            delete rolledBack[messageId];
          }
          return { contextNotes: rolledBack };
        });
        latestContextNoteGeneration.delete(messageId);
      }
      throw err;
    }
  },
}));
