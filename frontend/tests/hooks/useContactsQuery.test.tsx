import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { Contact } from "@/lib/api";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { listContacts } from "@/lib/api";
import {
  contactsQueryKey,
  useContactsQuery,
} from "@/hooks/queries/useContactsQuery";

vi.mock("@/lib/api", () => ({
  listContacts: vi.fn(() => Promise.resolve([])),
}));

const mockListContacts = vi.mocked(listContacts);

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

describe("useContactsQuery", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("includes search, favorite filter, limit, and offset in its query key", () => {
    expect(
      contactsQueryKey({ query: "alice", favoriteOnly: true, limit: 25, offset: 50 }),
    ).toEqual(["contacts", "alice", true, 25, 50]);
  });

  it("debounces changed search text for 200 milliseconds", async () => {
    vi.useFakeTimers();
    const { rerender } = renderHook(
      ({ query }) => useContactsQuery({ query, favoriteOnly: false, limit: 50, offset: 0 }),
      { initialProps: { query: "a" }, wrapper: createWrapper() },
    );
    await act(async () => Promise.resolve());
    expect(mockListContacts).toHaveBeenCalledWith("a", false, 50, 0);

    rerender({ query: "alice" });
    await act(async () => {
      vi.advanceTimersByTime(199);
      await Promise.resolve();
    });
    expect(mockListContacts).not.toHaveBeenCalledWith("alice", false, 50, 0);

    await act(async () => {
      vi.advanceTimersByTime(1);
      await Promise.resolve();
    });
    expect(mockListContacts).toHaveBeenCalledWith("alice", false, 50, 0);
  });

  it("loads every backend page when the requested limit exceeds 200", async () => {
    const contact = (index: number): Contact => ({
      id: `contact-${index}`,
      display_name: `Contact ${index}`,
      notes: "",
      is_favorite: false,
      emails: [{
        id: `email-${index}`,
        address: `user${index}@example.com`,
        label: "other",
        is_primary: true,
      }],
      created_at: 1,
      updated_at: 1,
    });
    mockListContacts.mockImplementation(async (_query, _favoriteOnly, _limit, offset) => (
      offset === 0
        ? Array.from({ length: 200 }, (_, index) => contact(index))
        : [contact(200)]
    ));

    const { result } = renderHook(
      () => useContactsQuery({ query: "", favoriteOnly: false, limit: 10_000, offset: 0 }),
      { wrapper: createWrapper() },
    );

    await waitFor(() => expect(result.current.data).toHaveLength(201));
    expect(mockListContacts).toHaveBeenNthCalledWith(1, "", false, 200, 0);
    expect(mockListContacts).toHaveBeenNthCalledWith(2, "", false, 200, 200);
  });
});
