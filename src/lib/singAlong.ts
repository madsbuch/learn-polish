/**
 * Sing along: the words of a song's transcript that become blanks to fill
 * while it plays (components/FollowLyrics.tsx). Pure, so the transcript can
 * work them out once per song and redraw cheaply many times a second.
 */
import type { GlossWord, TimedLine } from "./schemas";

/** A blank opens this many seconds before its line is sung… */
export const LEAD_S = 4;
/** …and blanks are at least this far apart, so each can be answered. */
const SPACING_S = 3;
/** A line shorter than this goes by too fast to fill. */
const MIN_LINE_S = 1;

/** One word of a line: what is shown around it, and the word itself. */
export interface LyricToken {
  /** Spaces and punctuation before the word. */
  before: string;
  /** The word as written ("Kawę"). */
  word: string;
  /** Punctuation after it. */
  after: string;
  /** Lowercased, for looking it up ("kawę"): empty for a dash alone. */
  form: string;
}

/** A line's words, in order, with the spaces and punctuation kept around them. */
export function tokens(text: string): LyricToken[] {
  const out: LyricToken[] = [];
  for (const match of text.matchAll(/(\s*)(\S+)/g)) {
    const [, space, chunk] = match;
    const core = chunk.match(/^([^\p{L}\p{N}]*)(.*?)([^\p{L}\p{N}]*)$/u);
    const lead = core?.[1] ?? "";
    const word = core?.[2] ?? "";
    out.push({ before: space + lead, word, after: core?.[3] ?? "", form: word.toLowerCase() });
  }
  return out;
}

/** Folded for comparing what was typed: no case, no Polish marks. */
export function fold(text: string): string {
  return text
    .toLowerCase()
    .replace(/ł/g, "l")
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .replace(/[^\p{L}\p{N}]/gu, "");
}

/** Short words that make poor blanks when nothing better is there. */
const SMALL_WORDS = new Set([
  "jest",
  "jestem",
  "jesteś",
  "się",
  "nie",
  "tak",
  "ale",
  "albo",
  "czy",
  "jak",
  "też",
  "tylko",
  "przez",
  "przy",
  "przed",
  "który",
  "która",
  "które",
  "mnie",
  "ciebie",
  "jego",
  "jej",
  "nas",
  "was",
  "ich",
  "tam",
  "tu",
  "tutaj",
  "bardzo",
  "teraz",
  "jeszcze",
  "już",
]);

export interface Blank {
  /** Which word of the line: an index into its tokens. */
  token: number;
  /** The word as written. */
  answer: string;
  /** Three to pick from, lowercase (a capital would give it away), the answer among them. */
  choices: string[];
}

/** A small seeded shuffle, so a line's choices stay put between redraws. */
function shuffleSeeded<T>(items: T[], seed: number): T[] {
  const out = [...items];
  let s = seed * 9301 + 49297;
  for (let i = out.length - 1; i > 0; i--) {
    s = (s * 9301 + 49297) % 233280;
    const j = Math.floor((s / 233280) * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

const letters = (word: string) => [...word].length;

/**
 * The blanks of a song: at most one per Polish line, spaced so each can be
 * answered in time. The word is one of the song's course words if the line
 * has one, else a word with a known meaning, else its longest content word.
 * The wrong choices are other blanks' words first (they sound like they
 * belong), then other words of the song.
 */
export function chooseBlanks(lines: TimedLine[], glossary: Map<string, GlossWord>): Map<number, Blank> {
  const picked = new Map<number, Omit<Blank, "choices">>();
  let lastEnd = -Infinity;
  lines.forEach((line, i) => {
    if (line.lang !== "pl" || line.end - line.start < MIN_LINE_S || line.start - lastEnd < SPACING_S) return;
    const words = tokens(line.text)
      .map((t, index) => ({ ...t, index }))
      .filter((t) => letters(t.form) >= 3);
    const longest = (list: typeof words) => [...list].sort((a, b) => letters(b.form) - letters(a.form))[0];
    const choice =
      words.find((t) => glossary.get(t.form)?.lesson) ??
      longest(words.filter((t) => glossary.has(t.form) && letters(t.form) >= 4 && !SMALL_WORDS.has(t.form))) ??
      longest(words.filter((t) => letters(t.form) >= 5 && !SMALL_WORDS.has(t.form)));
    if (!choice) return;
    picked.set(i, { token: choice.index, answer: choice.word });
    lastEnd = line.end;
  });

  const answers = [...picked.values()].map((b) => b.answer);
  const others = lines
    .filter((l) => l.lang === "pl")
    .flatMap((l) => tokens(l.text).map((t) => t.word))
    .filter((w) => letters(w) >= 4);
  const blanks = new Map<number, Blank>();
  for (const [i, blank] of picked) {
    const decoys: string[] = [];
    for (const candidate of [...shuffleSeeded(answers, i), ...shuffleSeeded(others, i + 7)]) {
      if (decoys.length === 2) break;
      if (fold(candidate) === fold(blank.answer) || decoys.some((d) => fold(d) === fold(candidate))) continue;
      decoys.push(candidate);
    }
    const choices = [blank.answer, ...decoys].map((w) => w.toLowerCase());
    blanks.set(i, { ...blank, choices: shuffleSeeded(choices, i + 3) });
  }
  return blanks;
}
