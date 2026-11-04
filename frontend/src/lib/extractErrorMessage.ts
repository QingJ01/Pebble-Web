/** Extract a human-readable error message from an unknown catch value. */
export function extractErrorMessage(err: unknown): string {
  const serverError = (err as { response?: { data?: { error?: unknown } } } | null)?.response?.data?.error;
  if (typeof serverError === "string" && serverError.trim()) return serverError;
  if (err instanceof Error) return err.message;
  if (typeof err === "string") return err;
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return "Unknown error";
}
