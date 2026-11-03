import api from "@/api-client";

export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  try {
    const response = await api.post<T>(`/commands/${encodeURIComponent(command)}`, args);
    return response.data;
  } catch (error: unknown) {
    const failure = error as { response?: { data?: { error?: unknown; message?: unknown } }; message?: string };
    const reason = failure.response?.data?.error ?? failure.response?.data?.message;
    throw new Error(typeof reason === "string" ? reason : failure.message ?? "Request failed");
  }
}
