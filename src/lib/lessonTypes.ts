/**
 * Lesson types, as the app's core registers them (src-tauri/src/lesson_types):
 * one flat list, loaded once at startup. Screens ask a lesson's type for its
 * name, icon, engine and page sections instead of branching on the type —
 * a new type shows up here without changing them.
 */
import { api } from "./api";
import type { Lesson, LessonTypeInfo } from "./types";

let types = new Map<string, LessonTypeInfo>();

/** Loads the registry; called once while the app starts. */
export async function loadLessonTypes(): Promise<void> {
  const list = await api.lessonTypes();
  types = new Map(list.map((type) => [type.id, type]));
}

/** A lesson's type; an unknown one reads as a plain drill or conversation. */
export function lessonType(lesson: Pick<Lesson, "type" | "engine">): LessonTypeInfo {
  return (
    types.get(lesson.type) ?? {
      id: lesson.type,
      name: lesson.engine === "conversation" ? "Conversation" : "Drill",
      icon: lesson.engine === "conversation" ? "study" : "drill",
      description: "",
      engine: lesson.engine,
      unaidedToMaster: false,
      sections: [],
      startLabel: "Start",
      againLabel: "Again",
      minutes: lesson.engine === "conversation" ? [5, 10, 20] : [2, 4, 8],
    }
  );
}

/** Every type, in the order they are offered. */
export function allLessonTypes(): LessonTypeInfo[] {
  return [...types.values()];
}
