import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  deleteContact,
  saveContact,
  setContactFavorite,
  type Contact,
  type ContactInput,
} from "@/lib/api";
import { useContactMutations } from "@/hooks/mutations/useContactMutations";

vi.mock("@/lib/api", () => ({
  deleteContact: vi.fn(() => Promise.resolve()),
  saveContact: vi.fn(),
  setContactFavorite: vi.fn(() => Promise.resolve()),
}));

const mockDeleteContact = vi.mocked(deleteContact);
const mockSaveContact = vi.mocked(saveContact);
const mockSetContactFavorite = vi.mocked(setContactFavorite);

const input: ContactInput = {
  display_name: "Alice",
  notes: "",
  is_favorite: false,
  emails: [{ address: "alice@example.com", label: "work", is_primary: true }],
};

const saved: Contact = {
  id: "contact-1",
  display_name: "Alice",
  notes: "",
  is_favorite: false,
  emails: [{
    id: "email-1",
    address: "alice@example.com",
    label: "work",
    is_primary: true,
  }],
  created_at: 1,
  updated_at: 1,
};

function setup() {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  });
  const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const hook = renderHook(() => useContactMutations(), { wrapper });
  return { ...hook, invalidateSpy };
}

function expectContactCachesInvalidated(invalidateSpy: ReturnType<typeof vi.spyOn>) {
  expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ["contacts"] });
  expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ["contact-suggestions"] });
}

describe("useContactMutations", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSaveContact.mockResolvedValue(saved);
  });

  it("invalidates contact and suggestion caches after saving", async () => {
    const { result, invalidateSpy } = setup();

    await act(async () => result.current.save.mutateAsync(input));

    expect(mockSaveContact).toHaveBeenCalledWith(input);
    await waitFor(() => expectContactCachesInvalidated(invalidateSpy));
  });

  it("invalidates contact and suggestion caches after deleting", async () => {
    const { result, invalidateSpy } = setup();

    await act(async () => result.current.remove.mutateAsync({
      contactId: "contact-1",
      suppressAddresses: true,
    }));

    expect(mockDeleteContact).toHaveBeenCalledWith("contact-1", true);
    await waitFor(() => expectContactCachesInvalidated(invalidateSpy));
  });

  it("invalidates contact and suggestion caches after changing favorite state", async () => {
    const { result, invalidateSpy } = setup();

    await act(async () => result.current.setFavorite.mutateAsync({
      contactId: "contact-1",
      isFavorite: true,
    }));

    expect(mockSetContactFavorite).toHaveBeenCalledWith("contact-1", true);
    await waitFor(() => expectContactCachesInvalidated(invalidateSpy));
  });
});
