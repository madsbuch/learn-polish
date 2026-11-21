/**
 * The microphone: recording the learner for speaking cards and for the mic
 * in the chat boxes (speaking.rs hears it).
 *
 * The web view records with getUserMedia and MediaRecorder, in whatever
 * the platform's recorder makes (WebM/Opus on Android's WebView, MP4/AAC in
 * WKWebView on the Mac and iPhone). Not every hearing model takes those, so
 * the take is decoded and sent as 16 kHz mono WAV — a format every one of
 * them reads — falling back to the recorder's own bytes should decoding
 * fail. A take stops itself after a short silence once something has been
 * said, or after a while with nothing said.
 *
 * Takes live in memory only: the original, to play back until the card is
 * left (`release`), and the WAV, sent once and dropped. Nothing is saved.
 *
 * Permission: Android asks the first time (the WebView's chrome client
 * requests RECORD_AUDIO, declared by scripts/prepare-android.mjs); macOS asks
 * with the usage string in src-tauri/Info.plist (and the audio-input
 * entitlement). Refused, recording fails with `MicError` "denied", and the
 * card says how to allow it.
 */

import { platform } from "@tauri-apps/plugin-os";

const os = (() => {
  try {
    return platform();
  } catch {
    return "";
  }
})();

export type MicProblem = "denied" | "missing" | "unsupported" | "failed";

/** Why the microphone could not be used. */
export class MicError extends Error {
  constructor(
    readonly problem: MicProblem,
    message: string,
  ) {
    super(message);
  }
}

/** What to say for each problem, and how to fix it on this device. */
export function micHelp(problem: MicProblem): string {
  switch (problem) {
    case "denied":
      if (os === "android") return "Polski isn't allowed to use the microphone. Allow it in Settings → Apps → Polski → Permissions → Microphone.";
      if (os === "ios") return "Polski isn't allowed to use the microphone. Allow it in Settings → Polski → Microphone.";
      if (os === "macos") return "Polski isn't allowed to use the microphone. Allow it in System Settings → Privacy & Security → Microphone.";
      return "The microphone isn't allowed. Allow it in your browser's site settings.";
    case "missing":
      return "No microphone was found. Plug one in, or skip speaking for now.";
    case "unsupported":
      return "Recording isn't available here. Skip speaking for now.";
    default:
      return "The microphone didn't start. Try again, or skip speaking for now.";
  }
}

/** Whether this web view can record at all. */
export function canRecord(): boolean {
  return typeof MediaRecorder !== "undefined" && typeof navigator.mediaDevices?.getUserMedia === "function";
}

/** The recorder's format: the first of these the platform has. */
function recorderType(): string {
  for (const type of ["audio/webm;codecs=opus", "audio/mp4", "audio/ogg;codecs=opus", "audio/webm", "audio/aac"]) {
    try {
      if (MediaRecorder.isTypeSupported(type)) return type;
    } catch {
      // An old recorder without isTypeSupported: its default will do.
    }
  }
  return "";
}

/** A recording, ready to hear and to send. */
export interface Take {
  /** For playing it back (an object URL: `release` it when done). */
  url: string;
  /** Base64 of the bytes sent, in `format` (OpenRouter's names). */
  audio: string;
  format: string;
  ms: number;
}

/** Frees a take's playback. */
export function release(take: Take | null | undefined) {
  if (take) URL.revokeObjectURL(take.url);
}

/** A recording in progress. */
export interface Recording {
  /** Stops and makes the take. */
  stop: () => Promise<Take>;
  /** Stops and throws it away. */
  cancel: () => void;
}

/** Below this loudness (RMS) is quiet, whatever the room's noise. */
const QUIET = 0.015;
/** The noisiest room the bar for speech is raised to. */
const LOUD_ROOM = 0.04;
/** Quiet this long after speaking stops the take… */
const SILENCE_MS = 1300;
/** …nothing said this long stops it too… */
const NOTHING_MS = 7000;
/** …and no take runs longer than this. */
const LONGEST_MS = 20000;
/** The sample rate sent: plenty for speech, a third of the bytes of 48 kHz. */
const RATE = 16000;

/**
 * Starts recording. `onLevel` hears how loud it is (0–1), for the button's
 * pulse; `onAutoStop` is called when the take stops itself (silence after
 * speech, or too long), so the caller can `stop()` it.
 */
export async function startRecording({
  onLevel,
  onAutoStop,
}: {
  onLevel?: (level: number) => void;
  onAutoStop?: () => void;
} = {}): Promise<Recording> {
  if (!canRecord()) throw new MicError("unsupported", micHelp("unsupported"));
  let stream: MediaStream;
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true, channelCount: 1 },
    });
  } catch (cause) {
    const name = cause instanceof DOMException ? cause.name : "";
    const problem: MicProblem =
      name === "NotAllowedError" || name === "SecurityError" || name === "PermissionDeniedError"
        ? "denied"
        : name === "NotFoundError" || name === "DevicesNotFoundError" || name === "OverconstrainedError"
          ? "missing"
          : "failed";
    throw new MicError(problem, micHelp(problem));
  }

  const type = recorderType();
  const recorder = type ? new MediaRecorder(stream, { mimeType: type }) : new MediaRecorder(stream);
  const chunks: Blob[] = [];
  recorder.addEventListener("dataavailable", (event) => {
    if (event.data.size > 0) chunks.push(event.data);
  });
  const started = performance.now();
  recorder.start(250);

  // Listening for silence: how loud, every 50 ms.
  const context = new AudioContext();
  const analyser = context.createAnalyser();
  analyser.fftSize = 1024;
  context.createMediaStreamSource(stream).connect(analyser);
  const samples = new Float32Array(analyser.fftSize);
  let heardAt = 0;
  let spoke = false;
  let floor = QUIET;
  let auto = false;
  const timer = window.setInterval(() => {
    analyser.getFloatTimeDomainData(samples);
    let sum = 0;
    for (const s of samples) sum += s * s;
    const rms = Math.sqrt(sum / samples.length);
    const now = performance.now();
    // The room's own noise, from the first moment, sets the bar for speech
    // — never so high that a voice starting at once is taken for the room.
    if (now - started < 300) floor = Math.min(LOUD_ROOM, Math.max(floor, rms * 2.5));
    onLevel?.(Math.min(1, rms * 8));
    if (rms > floor) {
      spoke = true;
      heardAt = now;
    }
    const quietFor = now - (spoke ? heardAt : started);
    if (!auto && ((spoke && quietFor > SILENCE_MS) || (!spoke && quietFor > NOTHING_MS) || now - started > LONGEST_MS)) {
      auto = true;
      onAutoStop?.();
    }
  }, 50);

  const finish = () => {
    window.clearInterval(timer);
    for (const track of stream.getTracks()) track.stop();
    void context.close().catch(() => undefined);
  };

  return {
    cancel: () => {
      finish();
      if (recorder.state !== "inactive") recorder.stop();
    },
    stop: () =>
      new Promise<Take>((resolve, reject) => {
        recorder.addEventListener(
          "stop",
          () => {
            finish();
            const blob = new Blob(chunks, { type: recorder.mimeType || type || "audio/webm" });
            const ms = Math.round(performance.now() - started);
            toSend(blob)
              .then(({ audio, format }) => resolve({ url: URL.createObjectURL(blob), audio, format, ms }))
              .catch((cause: unknown) => reject(cause instanceof Error ? cause : new Error(String(cause))));
          },
          { once: true },
        );
        if (recorder.state === "inactive") recorder.dispatchEvent(new Event("stop"));
        else recorder.stop();
      }),
  };
}

/** The take as sent: WAV where it decodes, else the recorder's bytes. */
async function toSend(blob: Blob): Promise<{ audio: string; format: string }> {
  const buffer = await blob.arrayBuffer();
  const bytes = new Uint8Array(buffer);
  try {
    const context = new AudioContext();
    try {
      // Decoding takes the buffer over: it gets a copy.
      const decoded = await context.decodeAudioData(buffer.slice(0));
      return { audio: base64(wav(mono(decoded), decoded.sampleRate)), format: "wav" };
    } finally {
      void context.close().catch(() => undefined);
    }
  } catch {
    const type = blob.type;
    const format = type.includes("mp4") || type.includes("aac") ? "m4a" : type.includes("ogg") ? "ogg" : "webm";
    return { audio: base64(bytes), format };
  }
}

/** All channels mixed to one. */
function mono(buffer: AudioBuffer): Float32Array {
  const out = new Float32Array(buffer.length);
  for (let c = 0; c < buffer.numberOfChannels; c++) {
    const data = buffer.getChannelData(c);
    for (let i = 0; i < out.length; i++) out[i] += data[i] / buffer.numberOfChannels;
  }
  return out;
}

/** 16-bit PCM WAV at `RATE`, resampled (linearly: it is speech, for a model). */
export function wav(samples: Float32Array, rate: number): Uint8Array {
  const length = Math.max(0, Math.floor((samples.length * RATE) / rate));
  const out = new Uint8Array(44 + length * 2);
  const view = new DataView(out.buffer);
  const text = (at: number, value: string) => {
    for (let i = 0; i < value.length; i++) view.setUint8(at + i, value.charCodeAt(i));
  };
  text(0, "RIFF");
  view.setUint32(4, 36 + length * 2, true);
  text(8, "WAVE");
  text(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, RATE, true);
  view.setUint32(28, RATE * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  text(36, "data");
  view.setUint32(40, length * 2, true);
  for (let i = 0; i < length; i++) {
    const at = (i * rate) / RATE;
    const j = Math.floor(at);
    const next = samples[Math.min(j + 1, samples.length - 1)] ?? 0;
    const value = (samples[j] ?? 0) + ((next - (samples[j] ?? 0)) * (at - j));
    view.setInt16(44 + i * 2, Math.max(-1, Math.min(1, value)) * 0x7fff, true);
  }
  return out;
}

function base64(bytes: Uint8Array): string {
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(text);
}
