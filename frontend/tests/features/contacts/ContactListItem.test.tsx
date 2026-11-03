import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ContactListItem from "@/features/contacts/ContactListItem";
import type { Contact } from "@/lib/api";

const contact: Contact = {
  id: "contact-1",
  display_name: "Alice Example",
  notes: "",
  is_favorite: true,
  created_at: 1,
  updated_at: 1,
  emails: [
    {
      id: "email-1",
      address: "secondary@example.com",
      label: "other",
      is_primary: false,
    },
    {
      id: "email-2",
      address: "alice@example.com",
      label: "work",
      is_primary: true,
    },
  ],
};

describe("ContactListItem", () => {
  it("renders the display name, primary email, initials, and favorite state", () => {
    render(<ContactListItem contact={contact} selected onSelect={vi.fn()} />);

    const button = screen.getByRole("button", { name: "Alice Example alice@example.com" });
    expect(button.getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByText("AE")).toBeTruthy();
    expect(screen.getByText("alice@example.com")).toBeTruthy();
    expect(document.querySelector(".contact-list-star")).toBeTruthy();
  });

  it("falls back to the first email and invokes selection", () => {
    const onSelect = vi.fn();
    render(
      <ContactListItem
        contact={{
          ...contact,
          display_name: "",
          is_favorite: false,
          emails: contact.emails.map((email) => ({ ...email, is_primary: false })),
        }}
        selected={false}
        onSelect={onSelect}
      />,
    );

    const button = screen.getByRole("button", {
      name: "secondary@example.com secondary@example.com",
    });
    expect(button.getAttribute("aria-pressed")).toBe("false");
    expect(screen.getByText("SE")).toBeTruthy();
    expect(document.querySelector(".contact-list-star")).toBeNull();

    fireEvent.click(button);
    expect(onSelect).toHaveBeenCalledOnce();
  });
});
