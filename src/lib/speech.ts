/**
 * Polish audio, from one of two sources:
 *
 *  1. OpenRouter, synthesised in Rust and cached in SQLite. Works everywhere,
 *     including Android's WebView, which does not implement `speechSynthesis`.
 *  2. The platform's own speech engine, when it has one. Free and instant, but
 *     absent on Android and silent without a Polish voice installed.
 *
 * The AI voice wins when it is configured, because it is the only option that
 * is guaranteed to work on the phone.
 */
import { api } from "./api";

let hasKey = false;
let hasVoice = false;

/** Kept in sync by the app shell whenever settings change. */
export function setSpeechConfig(config: { hasKey: boolean; hasVoice: boolean }): void {
  hasKey = config.hasKey;
  hasVoice = config.hasVoice;
}

const aiVoiceReady = () => hasKey && hasVoice;

/** Names the actual blocker, rather than blaming whichever engine ran last. */
function unavailableReason(): string {
  if (hasVoice && !hasKey) {
    return "Add your OpenRouter API key in Settings — the Polish voice needs it.";
  }
  if (hasKey && !hasVoice) {
    return "Choose a Polish voice in Settings to hear this.";
  }
  return "No speech on this device yet. Set up an OpenRouter voice in Settings to hear Polish.";
}

// ---------------------------------------------------------------- native path

let cachedVoice: SpeechSynthesisVoice | null | undefined;

function polishVoice(): SpeechSynthesisVoice | null {
  if (cachedVoice !== undefined) return cachedVoice;
  if (typeof speechSynthesis === "undefined") {
    cachedVoice = null;
    return null;
  }
  const voices = speechSynthesis.getVoices();
  // Voices load asynchronously on some platforms; don't cache an empty answer.
  if (voices.length === 0) return null;
  cachedVoice = voices.find((v) => v.lang.toLowerCase().startsWith("pl")) ?? null;
  return cachedVoice;
}

if (typeof speechSynthesis !== "undefined") {
  speechSynthesis.addEventListener("voiceschanged", () => {
    cachedVoice = undefined;
  });
}

function hasNativeSpeech(): boolean {
  return typeof speechSynthesis !== "undefined" && typeof SpeechSynthesisUtterance !== "undefined";
}

function speakNatively(text: string, rate: number): boolean {
  if (!hasNativeSpeech()) return false;
  try {
    speechSynthesis.cancel();
    const utterance = new SpeechSynthesisUtterance(text);
    utterance.lang = "pl-PL";
    utterance.rate = rate;
    const voice = polishVoice();
    if (voice) utterance.voice = voice;
    speechSynthesis.speak(utterance);
    return true;
  } catch {
    return false;
  }
}

// -------------------------------------------------------------------- ai path

// One element for the whole app, so starting a new clip stops the previous one.
let player: HTMLAudioElement | null = null;

function play(mime: string, base64: string): Promise<void> {
  if (!player) player = new Audio();
  player.pause();
  player.src = `data:${mime};base64,${base64}`;
  // Autoplay policies can reject this when there was no user gesture; the
  // caller treats that as "no audio" rather than an error worth showing.
  return player.play();
}

export function stop(): void {
  sequenceToken++;
  player?.pause();
  if (hasNativeSpeech()) {
    try {
      speechSynthesis.cancel();
    } catch {
      /* nothing playing */
    }
  }
}

/** True when *some* engine could produce audio for this device. */
export function canSpeak(): boolean {
  return aiVoiceReady() || hasNativeSpeech();
}

/** True when audio needs a network round trip the first time it is requested. */
export function usingAiVoice(): boolean {
  return aiVoiceReady();
}

export class SpeechError extends Error {}

/**
 * Speaks Polish. Resolves once playback has started; rejects only when the AI
 * voice was asked for and could not deliver, so the UI can explain why.
 */
export async function speak(text: string, rate = 0.85): Promise<void> {
  const line = text.trim();
  if (!line) return;

  if (aiVoiceReady()) {
    try {
      const clip = await api.speak(line);
      await play(clip.mime, clip.base64);
      return;
    } catch (cause) {
      // Fall back to the platform engine if there is one; otherwise report the
      // backend's message, which names the model or key that failed.
      if (speakNatively(line, rate)) return;
      throw new SpeechError(
        typeof cause === "string" ? cause : (cause as Error)?.message ?? "Could not play audio.",
      );
    }
  }

  if (!speakNatively(line, rate)) {
    throw new SpeechError(unavailableReason());
  }
}

/** Fire-and-forget speech for places that cannot show an error. */
export function speakQuietly(text: string, rate = 0.85): void {
  void speak(text, rate).catch(() => {
    /* the 🔊 button reports failures; auto-play stays silent */
  });
}

let sequenceToken = 0;

/**
 * Reads several lines back to back, waiting for each to finish. Playing a
 * conversation line by line reuses the same cache entries the individual 🔊
 * buttons fill, instead of paying for one long clip that duplicates them.
 */
export async function speakSequence(lines: string[], gapMs = 350): Promise<void> {
  const token = ++sequenceToken;
  for (const line of lines) {
    // A newer sequence, or a stop(), cancels this one.
    if (token !== sequenceToken) return;
    await speak(line);
    await finished();
    if (token !== sequenceToken) return;
    await new Promise((resolve) => setTimeout(resolve, gapMs));
  }
}

/** Resolves when the current clip stops, whichever engine is playing it. */
function finished(): Promise<void> {
  if (aiVoiceReady() && player) {
    const element = player;
    if (element.ended || element.paused) return Promise.resolve();
    return new Promise((resolve) => {
      const done = () => {
        element.removeEventListener("ended", done);
        element.removeEventListener("error", done);
        element.removeEventListener("pause", done);
        resolve();
      };
      element.addEventListener("ended", done);
      element.addEventListener("error", done);
      element.addEventListener("pause", done);
    });
  }
  if (!hasNativeSpeech()) return Promise.resolve();
  return new Promise((resolve) => {
    const poll = setInterval(() => {
      if (!speechSynthesis.speaking) {
        clearInterval(poll);
        resolve();
      }
    }, 120);
  });
}
