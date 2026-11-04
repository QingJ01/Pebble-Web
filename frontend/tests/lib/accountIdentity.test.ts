import { describe, expect, it } from "vitest";
import { accountLabel, accountOptionLabel, senderIdentityLabel } from "../../src/lib/accountIdentity";
import type { Account } from "../../src/lib/ipc-types";

const account: Account = { id: "a", email: "sender@example.com", display_name: "张三", account_label: "内部备用", provider: "imap", created_at: 1, updated_at: 1 };
describe("account identity presentation", () => {
  it("keeps local labels out of the sender identity and preserves the address", () => {
    expect(accountLabel(account)).toBe("内部备用");
    expect(accountOptionLabel(account)).toBe("内部备用 · sender@example.com");
    expect(senderIdentityLabel(account)).toBe("张三 <sender@example.com>");
    expect(accountOptionLabel({ ...account, account_label: "   " })).toBe(account.email);
  });
  it("uses only the provider name for Outlook, including when it is unknown", () => {
    expect(senderIdentityLabel({ ...account, provider: "outlook" })).toBe(account.email);
    expect(senderIdentityLabel({ ...account, provider: "outlook", provider_display_name: "Microsoft name" })).toBe("Microsoft name <sender@example.com>");
  });
});
