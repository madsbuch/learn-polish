import { useEffect, useRef, useState } from "react";
import { z } from "zod";

import { Banner, Button, Inline, Spinner, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { onEvent } from "../lib/events";
import { speakQuietly } from "../lib/speech";
import { toast } from "../lib/toast";
import { LessonIdSchema, type LessonDetail, type SceneSpot } from "../lib/types";
import { ScenePlaceholder, SceneView } from "./Scene";

const STATUS_TEXT = {
  drawing: "Drawing the scene…",
  locating: "Finding the things in it…",
} as const;

/**
 * A scene lesson's picture at the top of its page: tap a thing to hear it.
 * While it is being made, where it has got to (it changes as the core
 * reports, `scenes://changed`); if it failed, why, and a button to try
 * again. Once there, it can be drawn again, or its things found again.
 */
export function SceneHeader({ lesson, onChanged }: { lesson: LessonDetail; onChanged: () => void }) {
  const scene = lesson.scene;
  const [busy, setBusy] = useState<"draw" | "find" | null>(null);
  const [picked, setPicked] = useState<SceneSpot | null>(null);

  const changed = useRef(onChanged);
  changed.current = onChanged;
  useEffect(() => {
    const off = onEvent(z.object({ lessonId: LessonIdSchema, status: z.string() }), "scenes://changed", (event) => {
      if (event.payload.lessonId === lesson.id) changed.current();
    });
    return () => {
      void off.then((stop) => stop());
    };
  }, [lesson.id]);

  if (!scene) return null;

  async function again(redraw: boolean) {
    setBusy(redraw ? "draw" : "find");
    try {
      await api.sceneMake(lesson.id, redraw);
      onChanged();
    } catch (cause) {
      toast(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }

  const card = scene.card;
  const making = scene.status === "drawing" || scene.status === "locating";

  return (
    <div className="scene-header">
      {card && !making ? (
        <SceneView
          card={card}
          fit="page"
          description={scene.description}
          ring={(spot) => (picked?.wordId === spot.wordId ? "picked" : null)}
          label={(spot) =>
            picked?.wordId === spot.wordId ? (
              <>
                {spot.pl}
                <small>{spot.en}</small>
              </>
            ) : null
          }
          onTap={(spot) => {
            setPicked(spot);
            if (spot) speakQuietly(spot.pl);
          }}
        />
      ) : making ? (
        <ScenePlaceholder>
          <Spinner />
          <Text weight="bold">{STATUS_TEXT[scene.status === "drawing" ? "drawing" : "locating"]}</Text>
          <Text variant="small" tone="dim">
            {scene.working
              ? "It takes a minute. Until it's ready, the words are practised as a word drill."
              : "It is being made on the device that started it."}
          </Text>
        </ScenePlaceholder>
      ) : (
        <ScenePlaceholder>
          <Text weight="bold">No picture yet</Text>
        </ScenePlaceholder>
      )}

      {scene.status === "failed" ? (
        <Banner tone="error">
          <Inline gap={2} justify="between" wrap>
            <span>{scene.error || "The scene could not be made."}</span>
            <Button size="sm" loading={busy !== null} onClick={() => again(false)}>
              Try again
            </Button>
          </Inline>
        </Banner>
      ) : card && !making ? (
        <Inline gap={2} justify="between" wrap>
          <Text variant="small" tone="dim">
            Tap a thing to hear its name.
          </Text>
          <Inline gap={1}>
            <Button size="sm" variant="plain" loading={busy === "find"} disabled={busy !== null} onClick={() => again(false)}>
              Find things again
            </Button>
            <Button size="sm" variant="plain" loading={busy === "draw"} disabled={busy !== null} onClick={() => again(true)}>
              Draw again
            </Button>
          </Inline>
        </Inline>
      ) : null}
    </div>
  );
}
