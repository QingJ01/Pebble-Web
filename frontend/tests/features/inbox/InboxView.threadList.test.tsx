import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ThreadSummary } from "../../../src/lib/api";

const mockMailState = {
  activeAccountId: "account-1",
  activeFolderId: "folder-inbox",
  selectedMessageId: null as string | null,
  selectedThreadId: "thread-1" as string | null,
  threadView: true,
  setSelectedMessage: vi.fn(),
  setSelectedThreadId: vi.fn(),
  toggleThreadView: vi.fn(),
};

const threads: ThreadSummary[] = [{
  thread_id: "thread-1",
  subject: "Project update",
  snippet: "The latest project update",
  last_date: 1_700_000_000,
  message_count: 2,
  unread_count: 0,
  is_starred: false,
  participants: ["sender@example.com"],
  has_attachments: false,
}];

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

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 76,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({
        index,
        key: `row-${index}`,
        start: index * 76,
      })),
    measureElement: vi.fn(),
    scrollToIndex: vi.fn(),
  }),
}));

vi.mock("../../../src/stores/mail.store", () => ({
  useMailStore: (selector: (state: typeof mockMailState) => unknown) => selector(mockMailState),
}));

vi.mock("../../../src/hooks/queries", () => ({
  useAccountsQuery: () => ({ data: [{ id: "account-1" }] }),
  useFoldersForAccountsQuery: () => ({
    data: [{ id: "folder-inbox", role: "inbox" }],
  }),
  useMessagesQuery: () => ({
    data: [],
    isLoading: false,
    hasNextPage: false,
    isFetchingNextPage: false,
    fetchNextPage: vi.fn(),
  }),
  useThreadsQuery: () => ({
    data: threads,
    isLoading: false,
  }),
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

vi.mock("../../../src/components/ThreadItem", () => ({
  default: ({
    thread,
    isSelected,
    onClick,
  }: {
    thread: ThreadSummary;
    isSelected: boolean;
    onClick: () => void;
  }) => (
    <div role="option" aria-selected={isSelected} onClick={onClick}>
      {thread.subject}
    </div>
  ),
}));

vi.mock("../../../src/components/ConfirmDialog", () => ({
  default: () => <div>Confirm dialog</div>,
}));

vi.mock("../../../src/components/Skeleton", () => ({
  MessageListSkeleton: () => <div>Loading threads</div>,
}));

vi.mock("../../../src/lib/api", () => ({
  emptyTrash: vi.fn(),
}));

import InboxView from "../../../src/features/inbox/InboxView";

describe("InboxView thread list", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockMailState.activeAccountId = "account-1";
    mockMailState.activeFolderId = "folder-inbox";
    mockMailState.selectedMessageId = null;
    mockMailState.selectedThreadId = "thread-1";
    mockMailState.threadView = true;
  });

  it("groups virtualized thread options in a named listbox", () => {
    render(<InboxView />);

    const listbox = screen.getByRole("listbox", { name: "Threads" });
    expect(within(listbox).getByRole("option", { name: "Project update" })).toBeTruthy();
  });
});
