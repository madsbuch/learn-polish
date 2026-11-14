/**
 * Answer sounds: a bright rising chime when an answer is right, a soft low
 * bump when it isn't, so the ear learns the verdict before the eye reads it.
 *
 * They are synthesised with Web Audio — no files to ship, nothing to load,
 * and they play instantly. Whether they play is a per-device preference.
 */

const STORAGE_KEY = "polski.sounds";

let context: AudioContext | null = null;

function audio(): AudioContext | null {
  try {
    context ??= new AudioContext();
    if (context.state === "suspended") void context.resume();
    return context;
  } catch {
    return null;
  }
}

export function soundsEnabled(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) !== "off";
  } catch {
    return true;
  }
}

export function setSoundsEnabled(on: boolean) {
  try {
    localStorage.setItem(STORAGE_KEY, on ? "on" : "off");
  } catch {
    // Not stored: the choice lasts until the app closes.
  }
}

interface Note {
  /** Hz. */
  freq: number;
  /** Seconds after the sound starts. */
  at: number;
  /** Seconds the note rings for. */
  length: number;
  type?: OscillatorType;
  /** Peak gain, 0–1. */
  volume?: number;
  /** Slide to this frequency over the note (a little life). */
  glideTo?: number;
}

function play(notes: Note[]) {
  if (!soundsEnabled()) return;
  const ctx = audio();
  if (!ctx) return;
  const start = ctx.currentTime + 0.01;
  const master = ctx.createGain();
  master.gain.value = 0.5;
  master.connect(ctx.destination);
  for (const note of notes) {
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    const t = start + note.at;
    osc.type = note.type ?? "sine";
    osc.frequency.setValueAtTime(note.freq, t);
    if (note.glideTo) osc.frequency.exponentialRampToValueAtTime(note.glideTo, t + note.length);
    // A quick attack and an exponential fade: a struck, bell-like note.
    gain.gain.setValueAtTime(0.0001, t);
    gain.gain.exponentialRampToValueAtTime(note.volume ?? 0.4, t + 0.012);
    gain.gain.exponentialRampToValueAtTime(0.0001, t + note.length);
    osc.connect(gain).connect(master);
    osc.start(t);
    osc.stop(t + note.length + 0.05);
  }
}

/** Right: two quick rising notes, with a soft octave shimmer. */
export function playCorrect() {
  play([
    { freq: 784, at: 0, length: 0.18, type: "triangle", volume: 0.35 }, // G5
    { freq: 1175, at: 0.09, length: 0.38, type: "triangle", volume: 0.4 }, // D6
    { freq: 2350, at: 0.09, length: 0.3, volume: 0.08 },
  ]);
}

/** Wrong: a short, low, falling bump — noticeable, not punishing. */
export function playWrong() {
  play([
    { freq: 220, at: 0, length: 0.16, type: "square", volume: 0.12, glideTo: 196 },
    { freq: 165, at: 0.12, length: 0.28, type: "square", volume: 0.12, glideTo: 147 },
  ]);
}

/** A memory pair found: one small blip. */
export function playMatch() {
  play([{ freq: 1047, at: 0, length: 0.16, type: "triangle", volume: 0.3 }]);
}

/** A block finished well: a short rising flourish. */
export function playComplete() {
  play([
    { freq: 523, at: 0, length: 0.2, type: "triangle", volume: 0.3 }, // C5
    { freq: 659, at: 0.1, length: 0.2, type: "triangle", volume: 0.3 }, // E5
    { freq: 784, at: 0.2, length: 0.22, type: "triangle", volume: 0.32 }, // G5
    { freq: 1047, at: 0.3, length: 0.55, type: "triangle", volume: 0.38 }, // C6
    { freq: 2093, at: 0.3, length: 0.4, volume: 0.06 },
  ]);
}
