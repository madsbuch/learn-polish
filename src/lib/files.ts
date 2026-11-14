import { convertFileSrc } from "@tauri-apps/api/core";

/**
 * The URL of a stored file — a picture, a cover, a song — by its id in the
 * files table. The backend serves it through the `media` protocol straight
 * from the database (files.rs), with ranges for seeking. Always load files
 * this way, never by a path on disk.
 */
export function fileUrl(id: number): string {
  return convertFileSrc(String(id), "media");
}
