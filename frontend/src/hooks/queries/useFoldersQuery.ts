import { useMemo } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { listFolders } from "@/lib/api";

export const foldersQueryKey = (accountId: string) =>
  ["folders", accountId] as const;

export function useFoldersQuery(accountId: string | null) {
  return useQuery({
    queryKey: foldersQueryKey(accountId ?? ""),
    queryFn: () => listFolders(accountId!),
    enabled: !!accountId,
    select: (folders) => [...folders].sort((a, b) => a.sort_order - b.sort_order),
  });
}

export function useFoldersForAccountsQuery(accountIds: string[]) {
  const queries = useQueries({
    queries: accountIds.map((accountId) => ({
      queryKey: foldersQueryKey(accountId),
      queryFn: () => listFolders(accountId),
      enabled: !!accountId,
      staleTime: 15_000,
      refetchOnMount: "always",
    })),
  });

  const data = useMemo(
    () =>
      queries
        .flatMap((query) => query.data ?? [])
        .sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name)),
    [queries],
  );

  return {
    data,
    isFetched: accountIds.length === 0 || queries.every((query) => query.isFetched),
    isLoading: queries.some((query) => query.isLoading),
    isError: queries.some((query) => query.isError),
  };
}
