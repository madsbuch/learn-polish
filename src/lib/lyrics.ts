/**
 * Lyrics as plain text, so a draft can be edited in one textarea:
 *
 *   [Verse]
 *   Good morning | Dzień dobry
 *   [Chorus]
 *   A coffee with milk, please | Poproszę kawę z mlekiem
 *
 * English on the left, Polish on the right. Verses sing both, English first;
 * the chorus sings only the Polish.
 */
import type { SongSection, SongSectionKind } from "./types";

const LABELS: Record<SongSectionKind, string> = {
  verse: "Verse",
  chorus: "Chorus",
  bridge: "Bridge",
  intro: "Intro",
  outro: "Outro",
};

export function sectionLabel(kind: SongSectionKind): string {
  return LABELS[kind] ?? "Verse";
}

function kindOf(label: string): SongSectionKind {
  const lower = label.toLowerCase();
  if (lower.startsWith("chorus") || lower.startsWith("hook") || lower.startsWith("refrain"))
    return "chorus";
  if (lower.startsWith("bridge")) return "bridge";
  if (lower.startsWith("intro")) return "intro";
  if (lower.startsWith("outro")) return "outro";
  return "verse";
}

export function sectionsToText(sections: SongSection[]): string {
  return sections
    .map(
      (section) =>
        `[${sectionLabel(section.kind)}]\n` +
        section.lines.map((line) => `${line.en} | ${line.pl}`).join("\n"),
    )
    .join("\n\n");
}

export function textToSections(text: string): SongSection[] {
  const sections: SongSection[] = [];
  let current: SongSection | null = null;
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (!line) continue;
    const header = line.match(/^\[(.+)\]$/);
    if (header) {
      current = { kind: kindOf(header[1]), lines: [] };
      sections.push(current);
      continue;
    }
    if (!current) {
      current = { kind: "verse", lines: [] };
      sections.push(current);
    }
    const bar = line.indexOf("|");
    // A line without "|" is taken as Polish only.
    current.lines.push(
      bar < 0
        ? { en: "", pl: line }
        : { en: line.slice(0, bar).trim(), pl: line.slice(bar + 1).trim() },
    );
  }
  return sections.filter((section) => section.lines.length > 0);
}
