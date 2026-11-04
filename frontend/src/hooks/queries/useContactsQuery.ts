import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { getContactByEmail, listContacts, type Contact } from "@/lib/api";

export const contactsQueryRoot = ["contacts"] as const;
export const contactSuggestionsQueryRoot = ["contact-suggestions"] as const;

export interface ContactsQueryOptions {
  query: string;
  favoriteOnly: boolean;
  limit: number;
  offset: number;
}

export const contactsQueryKey = ({
  query,
  favoriteOnly,
  limit,
  offset,
}: ContactsQueryOptions) => ["contacts", query, favoriteOnly, limit, offset] as const;

export const contactByAddressQueryKey = (address: string) => [
  ...contactsQueryRoot,
  "by-address",
  address.trim().toLowerCase(),
] as const;

export const contactSuggestionsQueryKey = (accountId: string, query: string) =>
  ["contact-suggestions", accountId, query] as const;

const CONTACT_PAGE_SIZE = 200;

async function listContactPages(options: ContactsQueryOptions): Promise<Contact[]> {
  const requestedLimit = Math.max(1, options.limit);
  const contacts: Contact[] = [];
  let offset = Math.max(0, options.offset);

  while (contacts.length < requestedLimit) {
    const pageLimit = Math.min(CONTACT_PAGE_SIZE, requestedLimit - contacts.length);
    const page = await listContacts(
      options.query,
      options.favoriteOnly,
      pageLimit,
      offset,
    );
    contacts.push(...page);
    if (page.length < pageLimit) break;
    offset += page.length;
  }

  return contacts;
}

export function useContactsQuery(options: ContactsQueryOptions) {
  const [debouncedQuery, setDebouncedQuery] = useState(options.query);

  useEffect(() => {
    if (options.query === debouncedQuery) return;
    const timer = window.setTimeout(() => setDebouncedQuery(options.query), 200);
    return () => window.clearTimeout(timer);
  }, [debouncedQuery, options.query]);

  const debouncedOptions = { ...options, query: debouncedQuery };
  return useQuery({
    queryKey: contactsQueryKey(debouncedOptions),
    queryFn: () => listContactPages(debouncedOptions),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

export function useContactByAddressQuery(address: string, enabled = true) {
  const normalizedAddress = address.trim().toLowerCase();
  return useQuery({
    queryKey: contactByAddressQueryKey(normalizedAddress),
    queryFn: () => getContactByEmail(normalizedAddress),
    enabled: enabled && normalizedAddress.length > 0,
    staleTime: 5 * 60_000,
  });
}
