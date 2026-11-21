import { useEffect, useRef, useState } from "react";
import { z } from "zod";

import { Banner, Button, Inline, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { onEvent } from "../lib/events";
import { toast } from "../lib/toast";
import { LessonIdSchema, type LessonDetail, type PanelId } from "../lib/types";
import { PlayerIcon, StoryIcon } from "./icons";
import { StoryPicture } from "./Story";
import { StoryBook } from "./StoryBook";

/**
 * A story lesson's story at the top of its page: its first picture as a
 * cover, the picture book to read again (without the questions) or to
 * listen to, and every page as a thumbnail — drawn, being drawn (they fill
 * in as the core reports, `stories://changed`), or failed with its reason
 * and a button to draw it again.
 */
export function StoryHeader({ lesson, onChanged }: { lesson: LessonDetail; onChanged: () => void }) {
  const story = lesson.story;
  const [open, setOpen] = useState<{ listen: boolean; page: number } | null>(null);
  const [retrying, setRetrying] = useState<PanelId | "all" | null>(null);

  const changed = useRef(onChanged);
  changed.current = onChanged;
  useEffect(() => {
    const off = onEvent(z.object({ lessonId: LessonIdSchema, status: z.string() }), "stories://changed", (event) => {
      if (event.payload.lessonId === lesson.id) changed.current();
    });
    return () => {
      void off.then((stop) => stop());
    };
  }, [lesson.id]);

  if (!story) return null;

  async function redraw(panelId: PanelId | null) {
    setRetrying(panelId ?? "all");
    try {
      await api.storyDraw(lesson.id, panelId);
      onChanged();
    } catch (cause) {
      toast(errorMessage(cause), "error");
    } finally {
      setRetrying(null);
    }
  }

  const panels = story.panels;
  const ready = panels.filter((p) => p.status === "ready").length;
  const failed = panels.filter((p) => p.status === "failed").length;
  const making = panels.some((p) => p.status === "waiting" || p.status === "drawing");
  const cover = panels.find((p) => p.image) ?? panels[0];

  return (
    <div className="story-header">
      <div className="story-header__main">
        {cover ? (
          <button
            className="story-cover"
            onClick={() => setOpen({ listen: false, page: 0 })}
            aria-label={`Read “${lesson.title}”`}
          >
            <StoryPicture panel={cover} />
            <span className="story-cover__label">
              <StoryIcon />
              <span>{panels.length} pages</span>
            </span>
          </button>
        ) : null}
        <div className="story-header__side">
          <Inline gap={2} wrap>
            <Button variant="secondary" icon={<StoryIcon />} onClick={() => setOpen({ listen: false, page: 0 })}>
              Read the picture book
            </Button>
            <Button
              variant="plain"
              icon={<PlayerIcon.play size={14} />}
              onClick={() => setOpen({ listen: true, page: 0 })}
            >
              Listen to the whole story
            </Button>
          </Inline>

          {story.characters.length > 0 ? (
            <Text variant="small" tone="dim" as="p">
              {story.characters.map((c, i) => (
                <span key={c.name}>
                  {i > 0 ? " · " : ""}
                  <b>{c.name}</b>, {c.look}
                </span>
              ))}
            </Text>
          ) : null}

          {making ? (
            <Text variant="small" tone="dim">
              {story.working
                ? `Drawing the pictures: ${ready} of ${panels.length} ready. The story can be read meanwhile.`
                : `${ready} of ${panels.length} pictures ready — the rest are being drawn on the device that made the story.`}
            </Text>
          ) : null}
          {failed > 0 && !making ? (
            <Banner tone="error">
              <Inline gap={2} justify="between" wrap>
                <span>
                  {failed === 1 ? "One picture" : `${failed} pictures`} could not be drawn. The story reads without
                  {failed === 1 ? " it" : " them"}.
                </span>
                <Button size="sm" loading={retrying === "all"} onClick={() => void redraw(null)}>
                  Try again
                </Button>
              </Inline>
            </Banner>
          ) : null}

          <ol className="story-strip" aria-label="Pages">
            {panels.map((panel, i) => (
              <li key={panel.id}>
                <button
                  className="story-strip__page"
                  onClick={() => setOpen({ listen: false, page: i })}
                  aria-label={`Read from page ${panel.number}`}
                >
                  <StoryPicture panel={panel} small />
                </button>
                <Inline gap={1} justify="between">
                  <Text variant="caption" tone="faint">
                    {panel.number}
                  </Text>
                  {panel.status === "failed" ? (
                    <Button
                      size="sm"
                      variant="plain"
                      loading={retrying === panel.id}
                      onClick={() => void redraw(panel.id)}
                    >
                      Draw again
                    </Button>
                  ) : null}
                </Inline>
              </li>
            ))}
          </ol>
        </div>
      </div>

      {open && story ? (
        <StoryBook
          story={story}
          title={lesson.title}
          lessonId={lesson.id}
          listen={open.listen}
          startAt={open.page}
          onClose={() => setOpen(null)}
        />
      ) : null}
    </div>
  );
}
