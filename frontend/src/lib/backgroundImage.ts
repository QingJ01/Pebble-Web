import type { ImportedBackgroundImage } from "./ipc-types";

export async function importBackgroundImage(file: File): Promise<ImportedBackgroundImage> {
  if (!["image/png", "image/jpeg", "image/webp", "image/gif"].includes(file.type)) {
    throw new Error("Choose a PNG, JPEG, WebP or GIF image.");
  }
  if (file.size > 2 * 1024 * 1024) throw new Error("Background images must be smaller than 2 MiB.");
  const path = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error("Unable to read background image."));
    reader.onabort = () => reject(new Error("Image selection was cancelled."));
    reader.readAsDataURL(file);
  });
  return { path, filename: file.name, size: file.size };
}

export async function deleteBackgroundImage(path: string): Promise<void> {
  if (path.startsWith("blob:")) URL.revokeObjectURL(path);
}

export function backgroundImageUrl(path: string): string {
  return /^data:image\/(png|jpeg|webp|gif);base64,/i.test(path) ? path : "";
}
