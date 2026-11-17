import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { Button, Inline, Section, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { fileUrl } from "../lib/files";
import { toast } from "../lib/toast";
import type { LessonDetail } from "../lib/types";
import { SpeakButton } from "./ui";

/**
 * A picture lesson's pictures, one per word. Each can be drawn again or
 * removed; the missing ones can be drawn. Pictures arrive one by one as they are
 * drawn in the background, and the lesson reloads with each.
 */
export function WordPictures({ lesson, onChanged }: { lesson: LessonDetail; onChanged: () => void }) {
  const words = lesson.wordPictures;
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

  if (words.length === 0) return null;

  async function addAll() {
    setBusy("all");
    try {
      const count = await api.lessonAddPictures(lesson.id);
      setWaiting(count > 0);
      toast(
        count > 0
          ? `Drawing ${count} picture${count === 1 ? "" : "s"} — they appear here as they are done.`
          : "Every word has its picture.",
      );
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
      title={`${words.length} pictures`}
      info={
        <p>
          Each object is drawn once, in the same style, and kept on this device. A picture that does not show the word
          clearly can be drawn again, or removed: the word is then practised without one.
        </p>
      }
      action={
        missing > 0 ? (
          <Button size="sm" loading={busy === "all"} onClick={addAll}>
            {`Draw ${missing} missing`}
          </Button>
        ) : undefined
      }
    >
      <div className="word-pictures">
        {words.map((w) => (
          <div key={w.wordId} className="word-picture">
            {w.image !== null ? (
              <img src={fileUrl(w.image)} alt={w.meaning} />
            ) : (
              <div className="word-picture-empty">
                <Text variant="caption" tone="faint">
                  {waiting ? "Being drawn…" : "Not drawn yet"}
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
    </Section>
  );
}
