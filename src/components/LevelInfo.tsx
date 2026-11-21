import { List, ListItem, Sheet, Stack, Text } from "../design";

/** What each CEFR level means, in everyday terms. */
const LEVELS: { level: string; name: string; can: string }[] = [
  {
    level: "A1",
    name: "Beginner",
    can: "Greetings, names, numbers, ordering a coffee: short, set phrases about yourself and what is right in front of you.",
  },
  {
    level: "A2",
    name: "Elementary",
    can: "Everyday situations — shopping, directions, your work and family — in simple sentences, mostly in the present.",
  },
  {
    level: "B1",
    name: "Intermediate",
    can: "Getting by on a trip, telling what happened and what you plan, giving a short opinion; past and future tenses, linked sentences.",
  },
  {
    level: "B2",
    name: "Upper intermediate",
    can: "Following a conversation at normal speed, arguing a point, reading the news; most grammar, a broad vocabulary.",
  },
  {
    level: "C1",
    name: "Advanced",
    can: "Fluent and flexible: complex texts, nuance, humour, the language of work and study.",
  },
  { level: "C2", name: "Proficient", can: "Close to a native speaker: almost anything, precisely, in any register." },
];

/**
 * What a level means: the CEFR scale with this one marked, how the app sets
 * it, and that it never limits what can be made or done.
 */
export function LevelInfo({ level, what, onClose }: { level: string; what: string; onClose: () => void }) {
  return (
    <Sheet title={`${level} — what it means`} onClose={onClose}>
      <Stack gap={4}>
        <Text as="p">
          {`This ${what} is at ${level} on the CEFR scale — the common European scale for language levels, from A1 (a first course) to C2 (near native). It describes how advanced the material is: its words, its grammar and what its lessons ask you to do.`}
        </Text>
        <List>
          {LEVELS.map((entry) => (
            <ListItem
              key={entry.level}
              title={`${entry.level} · ${entry.name}${entry.level === level ? "  ←" : ""}`}
              subtitle={entry.can}
            />
          ))}
        </List>
        <Text variant="small" tone="dim" as="p">
          The tutor gives each course a level when it makes it, and a quick check reads the course to confirm it. A
          level never limits anything: ask the tutor for a course at any level, harder or easier, and it makes exactly
          that.
        </Text>
      </Stack>
    </Sheet>
  );
}
