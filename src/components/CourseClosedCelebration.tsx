import { useMemo } from "react";

import { Button, Stack, Text } from "../design";
import type { CourseClosed } from "../lib/types";

const COLOURS = ["var(--gold)", "var(--brand)", "var(--mint)", "var(--sky)", "var(--crimson)"];

/**
 * Closing a mastered course: the learner's own celebration. A trophy rises,
 * confetti falls, and the course's story is told in numbers. Calm (no
 * motion) when the system asks for reduced motion.
 */
export function CourseClosedCelebration({ closed, onDone }: { closed: CourseClosed; onDone: () => void }) {
  const pieces = useMemo(
    () =>
      Array.from({ length: 48 }, (_, i) => ({
        x: `${(i * 37) % 100}%`,
        delay: `${((i * 53) % 90) / 100}s`,
        spin: `${(i * 71) % 360}deg`,
        colour: COLOURS[i % COLOURS.length],
      })),
    [],
  );
  const stats: [string, string][] = [
    [String(closed.lessons), closed.lessons === 1 ? "lesson" : "lessons"],
    [String(closed.words), closed.words === 1 ? "word" : "words"],
    [String(closed.days), closed.days === 1 ? "day" : "days"],
    [String(closed.xp), "XP"],
  ];
  return (
    <div className="course-closed" role="dialog" aria-modal="true" aria-label={`${closed.title} mastered`}>
      <div className="course-closed-confetti" aria-hidden="true">
        {pieces.map((piece, i) => (
          <i
            key={i}
            style={
              {
                "--x": piece.x,
                "--delay": piece.delay,
                "--spin": piece.spin,
                "--colour": piece.colour,
              } as React.CSSProperties
            }
          />
        ))}
      </div>
      <Stack gap={5} align="center">
        <div className="course-closed-trophy" aria-hidden="true">
          🏆
        </div>
        <Stack gap={2} align="center">
          <Text variant="caption" tone="dim">
            Mastered
          </Text>
          <Text variant="title" align="center" as="h2">
            {closed.title}
          </Text>
        </Stack>
        <div className="course-closed-stats">
          {stats.map(([value, label]) => (
            <Stack key={label} gap={1} align="center">
              <Text variant="title">{value}</Text>
              <Text variant="caption" tone="dim">
                {label}
              </Text>
            </Stack>
          ))}
        </div>
        <Text variant="small" tone="dim" align="center" as="p">
          It's in Mastered now, for good. To come back to this theme, ask the tutor for a new course — a level up.
        </Text>
        <Button variant="primary" wide onClick={onDone}>
          Done
        </Button>
      </Stack>
    </div>
  );
}
