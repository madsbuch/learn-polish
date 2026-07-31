/** Polish text-to-speech through the platform webview. Availability varies by
 *  device, so every entry point degrades to a no-op rather than throwing. */

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

export function canSpeak(): boolean {
  return typeof speechSynthesis !== "undefined";
}

export function speak(text: string, rate = 0.85): void {
  if (typeof speechSynthesis === "undefined" || !text.trim()) return;
  try {
    speechSynthesis.cancel();
    const utterance = new SpeechSynthesisUtterance(text);
    utterance.lang = "pl-PL";
    utterance.rate = rate;
    const voice = polishVoice();
    if (voice) utterance.voice = voice;
    speechSynthesis.speak(utterance);
  } catch {
    // A missing engine should never break the exercise flow.
  }
}
