import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Account, Folder } from "../../../src/lib/api";

const mocks = vi.hoisted(() => ({
  mailState: {
    activeAccountId: null as string | null,
    activeFolderId: "all:inbox" as string | null,
    selectedMessageId: null as string | null,
    selectedThreadId: null as string | null,
    threadView: false,
    setSelectedMessage: vi.fn(),
    setSelectedThreadId: vi.fn(),
    toggleThreadView: vi.fn(),
  },
  accounts: [
    { id: "account-1", email: "one@example.com" },
    { id: "account-2", email: "two@example.com" },
  ] as Account[],
  folders: [
    { id: "a1-inbox", account_id: "account-1", role: "inbox", sort_order: 0 },
    { id: "a2-inbox", account_id: "account-2", role: "inbox", sort_order: 0 },
  ] as Folder[],
  useFoldersForAccountsQuery: vi.fn(),
  useMessagesQuery: vi.fn(),
  useThreadsQuery: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  initReactI18next: {
    type: "3rdParty",
    init: vi.fn(),
  },
  useTranslation: () => ({
    t: (_key: string, fallback?: string) => fallback ?? _key,
  }),
}));

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: vi.fn(),
  }),
}));

vi.mock("../../../src/stores/mail.store", () => ({
  useMailStore: (selector: (state: typeof mocks.mailState) => unknown) => selector(mocks.mailState),
}));

vi.mock("../../../src/stores/ui.store", () => ({
  useUIStore: (selector: (state: { setActiveView: () => void }) => unknown) =>
    selector({ setActiveView: vi.fn() }),
}));

vi.mock("../../../src/stores/toast.store", () => ({
  useToastStore: (selector: (state: { addToast: () => void }) => unknown) =>
    selector({ addToast: vi.fn() }),
}));

vi.mock("../../../src/hooks/queries", () => ({
  useAccountsQuery: () => ({ data: mocks.accounts }),
  useFoldersQuery: () => ({
    data: mocks.folders.slice(0, 1),
    isFetched: true,
  }),
  useFoldersForAccountsQuery: mocks.useFoldersForAccountsQuery,
  useMessagesQuery: mocks.useMessagesQuery,
  useThreadsQuery: mocks.useThreadsQuery,
  patchMessagesCache: vi.fn(),
}));

vi.mock("../../../src/components/SearchBar", () => ({
  default: () => <div>Search bar</div>,
}));

vi.mock("../../../src/components/MessageList", () => ({
  default: () => <div>Message list</div>,
}));

vi.mock("../../../src/components/MessageDetail", () => ({
  default: () => <div>Message detail</div>,
}));

vi.mock("../../../src/features/inbox/ThreadView", () => ({
  default: () => <div>Thread detail</div>,
}));

vi.mock("../../../src/components/ConfirmDialog", () => ({
  default: () => <div>Confirm dialog</div>,
}));

vi.mock("../../../src/lib/api", () => ({
  emptyTrash: vi.fn(),
}));

import InboxView from "../../../src/features/inbox/InboxView";

describe("InboxView all accounts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.mailState.activeAccountId = null;
    mocks.mailState.activeFolderId = "all:inbox";
    mocks.mailState.selectedMessageId = null;
    mocks.mailState.selectedThreadId = null;
    mocks.mailState.threadView = false;
    mocks.useFoldersForAccountsQuery.mockReturnValue({
      data: mocks.folders,
      isFetched: true,
    });
    mocks.useMessagesQuery.mockReturnValue({
      data: [],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    mocks.useThreadsQuery.mockReturnValue({
      data: [],
      isLoading: false,
    });
  });

  it("queries all matching inbox folders when all accounts are selected", () => {
    render(<InboxView />);

    expect(mocks.useFoldersForAccountsQuery).toHaveBeenCalledWith(["account-1", "account-2"]);
    expect(mocks.useMessagesQuery).toHaveBeenCalledWith("a1-inbox", ["a1-inbox", "a2-inbox"]);
  });
});
