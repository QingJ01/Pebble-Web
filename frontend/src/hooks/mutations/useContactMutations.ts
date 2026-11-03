import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  deleteContact,
  saveContact,
  setContactFavorite,
  type ContactInput,
} from "@/lib/api";
import { contactsQueryRoot, contactSuggestionsQueryRoot } from "@/hooks/queries/useContactsQuery";

async function invalidateContactCaches(queryClient: ReturnType<typeof useQueryClient>) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: contactsQueryRoot }),
    queryClient.invalidateQueries({ queryKey: contactSuggestionsQueryRoot }),
  ]);
}

export function useContactMutations() {
  const queryClient = useQueryClient();

  const save = useMutation({
    mutationFn: (input: ContactInput) => saveContact(input),
    onSuccess: async () => {
      await invalidateContactCaches(queryClient);
    },
  });

  const remove = useMutation({
    mutationFn: ({
      contactId,
      suppressAddresses = false,
    }: {
      contactId: string;
      suppressAddresses?: boolean;
    }) => deleteContact(contactId, suppressAddresses),
    onSuccess: async () => {
      await invalidateContactCaches(queryClient);
    },
  });

  const setFavorite = useMutation({
    mutationFn: ({
      contactId,
      isFavorite,
    }: {
      contactId: string;
      isFavorite: boolean;
    }) => setContactFavorite(contactId, isFavorite),
    onSuccess: async () => {
      await invalidateContactCaches(queryClient);
    },
  });

  return { save, remove, setFavorite };
}
