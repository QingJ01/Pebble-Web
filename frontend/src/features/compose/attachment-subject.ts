export function subjectAfterAddingAttachments(
  currentSubject: string,
  existingAttachmentCount: number,
  newAttachmentNames: readonly string[],
): string {
  if (currentSubject.trim() || existingAttachmentCount > 0) return currentSubject;
  return newAttachmentNames[0] ?? currentSubject;
}
