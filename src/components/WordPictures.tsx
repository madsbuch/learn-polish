import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { Button, Inline, Section, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { fileUrl } from "../lib/files";
import { toast } from "../lib/toast";
import type { LessonDetail } from "../lib/types";
import { SpeakButton } from "./ui";

/**
 * A lesson's pictures: every word of a picture lesson, or the nouns of a word
 * drill that was given pictures. Each can be drawn again or removed; Add
 * pictures draws the ones missing. Pictures arrive one by one as they are
 * drawn in the background, and the lesson reloads with each.
 */
export function WordPictures({ lesson, onChanged }: { lesson: LessonDetail; onChanged: () => void }) {
  const pictureLesson = lesson.format === "pictures";
  const words = lesson.wordPictures.filter((w) => pictureLesson || w.picturable || w.image !== null);
  const missing = words.filter((w) => w.image === null).length;
  const [busy, setBusy] = useState<number | "all" | null>(null);
  const [waiting, setWaiting] = useState(false);

  const changed = useRef(onChanged);
  changed.current = onChanged;
  useEffect(() => {
    const off = listen<{ lessonId: number }>("pictures://word", (event) => {
      if (event.payload.lessonId === lesson.id) changed.current();
    });
    return () => {
      void off.then((stop) => stop());
    };
  }, [lesson.id]);

  // Nothing pictured yet in a word drill: just the offer.
  const pictured = words.some((w) => w.image !== null);
  if (words.length === 0 || (!pictureLesson && !pictured && !waiting && missing === 0)) return null;

  async function addAll() {
    setBusy("all");
    try {
      const count = await api.lessonAddPictures(lesson.id);
      setWaiting(count > 0);
      toast(count > 0 ? `Drawing ${count} picture${count === 1 ? "" : "s"} — they appear here as they are done.` : "Every word has its picture.");
    } catch (cause) {
      toast(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }

  async function act(wordId: number, run: () => Promise<unknown>) {
    setBusy(wordId);
    try {
      await run();
      onChanged();
    } catch (cause) {
      toast(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }

  return (
    <Section
      title={pictureLesson ? `${words.length} pictures` : "Pictures"}
      info={
        <p>
          Each object is drawn once, in the same style, and kept on this device. A picture that does
          not show the word clearly can be drawn again, or removed: the word is then practised
          without one.
        </p>
      }
      action={
        missing > 0 ? (
          <Button size="sm" loading={busy === "all"} onClick={addAll}>
            {pictured ? `Draw ${missing} missing` : "Add pictures"}
          </Button>
        ) : undefined
      }
    >
      {!pictured && !pictureLesson ? (
        <Text variant="small" tone="dim" as="p">
          {`Give this drill's ${missing} object${missing === 1 ? "" : "s"} a picture each, to see them as you learn them.`}
        </Text>
      ) : (
        <div className="word-pictures">
          {words.map((w) => (
            <div key={w.wordId} className="word-picture">
              {w.image !== null ? (
                <img src={fileUrl(w.image)} alt={w.meaning} />
              ) : (
                <div className="word-picture-empty">
                  <Text variant="caption" tone="faint">
                    {waiting || pictureLesson ? "Being drawn…" : "No picture"}
                  </Text>
                </div>
              )}
              <Inline gap={1} justify="between">
                <Text variant="small" weight="bold" selectable>
                  {w.word}
                </Text>
                <SpeakButton text={w.word} size="sm" />
              </Inline>
              <Inline gap={1} wrap>
                <Button
                  size="sm"
                  loading={busy === w.wordId}
                  disabled={busy !== null}
                  onClick={() => act(w.wordId, () => api.wordPictureDraw(w.wordId))}
                >
                  {w.image !== null ? "Draw again" : "Draw"}
                </Button>
                {w.image !== null ? (
                  <Button
                    size="sm"
                    variant="plain"
                    disabled={busy !== null}
                    onClick={() => act(w.wordId, () => api.wordPictureDelete(w.wordId))}
                  >
                    Remove
                  </Button>
                ) : null}
              </Inline>
            </div>
          ))}
        </div>
      )}
    </Section>
  );
}
