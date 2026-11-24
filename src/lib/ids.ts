import { useEffect, useState } from "react";

import { api } from "./api";
import type { CourseId, LessonId, ResolvedId, SongId } from "./schemas";
import { CourseIdSchema, LessonIdSchema, SongIdSchema } from "./schemas";

/**
 * Tagged ids in text (DATA.md): a prefix, an underscore, 26 base32
 * characters. People never read them: the screens show the title instead.
 */
export const TYPE_ID =
  /\b(crs|ls|wd|sn|dl|sg|tk|pl|pe|fl|ci|cm|cv|msg|mem|src|xp|cost|bench)_[0-7][0-9a-hjkmnp-tv-z]{25}\b/;
const TYPE_IDS = new RegExp(TYPE_ID.source, "g");

/** Every TypeID in a text, once each. */
export function idsIn(text: string): string[] {
  return [...new Set(text.match(TYPE_IDS) ?? [])];
}

/** An icon for each kind, by prefix. */
export const KIND_ICON: Record<string, string> = {
  crs: "📚",
  ls: "📘",
  wd: "🔤",
  sn: "💬",
  dl: "🗨️",
  sg: "🎵",
  tk: "🎵",
  pl: "🎶",
  fl: "📎",
  ci: "✅",
  cv: "✨",
  msg: "✨",
  mem: "🧠",
  src: "🌐",
};

// Titles already resolved, and lookups in flight, shared by every screen.
const titles = new Map<string, ResolvedId>();
const waiting = new Map<string, Promise<void>>();

/** Resolves ids to their titles (one request for all the new ones). */
export async function resolveIds(ids: string[]): Promise<void> {
  const fresh = ids.filter((id) => !titles.has(id) && !waiting.has(id));
  if (fresh.length > 0) {
    const request = api
      .idsResolve(fresh)
      .then((found) => {
        for (const entry of found) titles.set(entry.id, entry);
      })
      .catch(() => undefined)
      .finally(() => fresh.forEach((id) => waiting.delete(id)));
    fresh.forEach((id) => waiting.set(id, request));
  }
  await Promise.all(
    ids
      .map((id) => waiting.get(id))
      .filter((p): p is Promise<void> => p !== undefined),
  );
}

/** What an id names, once resolved. */
export function titleOf(id: string): ResolvedId | undefined {
  return titles.get(id);
}

/** The title of an id, resolving it first if needed (null until then). */
export function useIdTitle(id: string): ResolvedId | null {
  const [entry, setEntry] = useState<ResolvedId | null>(titles.get(id) ?? null);
  useEffect(() => {
    let live = true;
    if (!titles.has(id))
      void resolveIds([id]).then(
        () => live && setEntry(titles.get(id) ?? null),
      );
    return () => {
      live = false;
    };
  }, [id]);
  return entry;
}

/** "ls_01jd… (Pierwsze spotkanie)": an id with its title, for text people copy. */
export function withTitles(text: string): string {
  return text.replace(TYPE_IDS, (id) => {
    const entry = titles.get(id);
    return entry?.title ? `${id} (${entry.title})` : id;
  });
}

/** What a chip opens, by kind: given by the app, so any screen's chips work. */
export interface IdOpeners {
  course?: (id: CourseId) => void;
  lesson?: (id: LessonId) => void;
  song?: (id: SongId) => void;
}

let openers: IdOpeners = {};

/** The app says how it opens things, once (chips on every screen use it). */
export function setIdOpeners(given: IdOpeners): void {
  openers = given;
}

/** How to open an id, if it is a kind the app opens. */
export function useOpener(id: string): (() => void) | null {
  const course = CourseIdSchema.safeParse(id);
  if (course.success && openers.course)
    return () => openers.course?.(course.data);
  const lesson = LessonIdSchema.safeParse(id);
  if (lesson.success && openers.lesson)
    return () => openers.lesson?.(lesson.data);
  const song = SongIdSchema.safeParse(id);
  if (song.success && openers.song) return () => openers.song?.(song.data);
  return null;
}
