# Polski

A Polish learning app for people who speak **English** or **Danish**. One
check-in a day: today's conversation, the words that belong to it, and its
sentences.

Built with Tauri 2 (Rust + SQLite) and React, packaged as a native app for
**Android** and **macOS**.

---

## What's in it

| | |
|---|---|
| **2007 words** | Across ten topics, each with an English *and* Danish gloss and a part of speech |
| **200 sentences** | Twenty per topic, translated into both languages |
| **10 conversations** | 207 lines of natural dialogue, one per topic |

The ten topics: basics & greetings · family & people · food & restaurant ·
shopping & money · travel & directions · home & daily life · work & study ·
health & body · time, weather & nature · leisure, media & technology.

All of it is compiled into the binary, so a fresh install works offline and
there is nothing to download.

## The daily check-in

Each day the app serves one conversation plus everything attached to it:

1. **Read the conversation** — tap any line to reveal its translation, or play it.
2. **Meet new words** from that conversation's topic, then recognise them.
3. **Review whatever is due** from earlier days, mixed in.
4. **Build sentences** from shuffled words.
5. **Fill gaps in the conversation** you just read.

Finish it and the streak advances; tomorrow brings the next conversation. The
ten conversations rotate, so the vocabulary keeps cycling while the spaced
repetition schedule decides what actually needs your attention.

### Scheduling

An SM-2 style scheduler (`src-tauri/src/srs.rs`) tracks every word and sentence
independently. Exercise difficulty follows the review history: brand-new items
are introduced on a card, then drilled with multiple choice; once an item has
survived a few correct reps you have to type it from memory. Typing is forgiving
about diacritics and case — `czesc` counts as `cześć`.

### Gamification

XP per answer (harder exercise types pay more, wrong answers pay nothing),
levels on a widening curve, a daily XP goal ring, a streak that survives until
the end of the following day, per-topic mastery, and fifteen achievements.

## AI tutor (optional)

Add an [OpenRouter](https://openrouter.ai/keys) API key in Settings and you get:

- a conversation partner that is briefed on today's topic and its vocabulary,
- corrections of the Polish you write,
- grammar explanations for any word or sentence, straight from the exercise
  you just got wrong,
- extra drills generated from the words you keep forgetting.

Answers **stream in** as they are generated, so a long explanation is readable
from the first line rather than after the last, and they are rendered as
markdown — headings, bold, lists and code, not raw `**asterisks**`. The
renderer builds React elements rather than HTML, so model output can never
inject markup and there is no sanitiser to keep in step.

**Every HTTP request is made from Rust**, in `src-tauri/src/ai.rs`, using
`reqwest`. The webview never issues a cross-origin request, so CORS never comes
into it, and the API key never reaches JavaScript — it lives in the app's
private SQLite database and is only ever read inside a command.

**No model is hardcoded.** Model ids go stale, so the app ships without a
default: press *Load models from OpenRouter* in Settings and pick one from the
live catalogue. Until you do, the AI features stay off and say so.

## Songs (optional)

Songs are one of the best ways to get Polish stuck in your head. The **Songs**
tab makes them to order:

- **The shape:** each verse pairs a short English line with the same line in
  Polish, sung right after it. The chorus is a single Polish sentence, sung only
  in Polish, so it is the thing you end up humming.
- **The songwriter** is an AI model on OpenRouter (its own model choice in
  Settings, or the tutor's). Give it a theme, a vocabulary topic and,
  optionally, the exact chorus sentence. It drafts title, style and lyrics from
  your words, leaning on the ones you keep forgetting. Edit the lyrics by hand
  (`English | Polish` per line) or ask it for changes before anything is sung.
- **Suno sings it** through [kie.ai](https://kie.ai) — add a kie.ai key in
  Settings and pick the Suno model (V6, V6 Mini or V6 Wild). Each song comes
  in two takes. Generation runs in Rust in the background and survives leaving
  the screen or restarting the app.
- **Songs are kept.** kie.ai deletes generated files after 14 days, so both
  takes and their cover art are downloaded into the app's data folder and play
  from disk, offline, for good.
- **Built for repetition:** play all, shuffle, repeat all / one, play counts,
  and a mini player above the tab bar so the music keeps going while you
  practise. Lock-screen controls use the Media Session API where the webview
  supports it.

## Hearing Polish

Every Polish line has a 🔊 button, and conversations have a ▶︎ that reads them
line by line. Audio comes from whichever of these is available:

1. **Gemini, through OpenRouter.** Speech is pinned to
   `google/gemini-3.1-flash-tts-preview`: it handles Polish well, and pinning
   it means the voice list ships with the app instead of needing a network
   round trip to fill a dropdown. Synthesis happens in Rust through
   `POST /api/v1/audio/speech`.

   Gemini only accepts `response_format: pcm`, and other providers only accept
   MP3, and neither says so until it rejects the request — so `synthesize`
   walks a short ladder: MP3 first, then PCM, then again without `speed` for
   models that do not honour it. PCM comes back with no container, so it is
   wrapped in a WAV header built from the rate and channel count in the
   response's `Content-Type`.

   **Conversations are read in two voices.** Every speaker in the ten
   dialogues is tagged female or male in the content files, and each line is
   synthesised in the matching voice — so the shop assistant and the customer
   do not sound like the same person. Both voices are chosen in Settings from
   Gemini's thirty, each with a preview button that speaks a Polish sentence.

   Not every TTS model speaks Polish at all: `hexgrad/kokoro-82m` renders
   *"Dzień dobry! Uczę się języka polskiego."* as *"Design dobry. Ach so,
   Jesika polskiego."* Gemini and `x-ai/grok-voice-tts-1.0` both come back
   verbatim, which is why Gemini is the one that is pinned.
2. **The platform's own engine**, when the webview has one. Free and instant,
   but Android's WebView does not implement `speechSynthesis` at all, and macOS
   needs a Polish system voice installed (System Settings → Accessibility →
   Spoken Content → Manage Voices → Polish).

The OpenRouter voice wins when configured, because it is the only option that
works identically on both platforms.

### Audio is cached

Each line is generated **once** and stored in SQLite as a blob, keyed by the
exact text plus the model and voice that produced it. After that it replays
instantly, for free, offline. Changing voice does not serve stale audio.

- Starting a session pre-fetches that day's lines in the background — dialogue
  lines in the voice that will actually read them — so no tap ever waits on
  the network. Toggleable in Settings.
- *Download every sentence and dialogue* warms all 200 sentences and all ten
  conversations in one pass, with progress reported line by line.
- Settings shows how many of the 200 sentences are stored and how much disk
  they take, with a button to clear them.

## Notifications

A daily reminder at a time you choose, scheduled through
`tauri-plugin-notification`. It is re-armed on every launch, because the OS
drops scheduled notifications after a reboot or an app update.

---

## Development

Requires [Bun](https://bun.sh) and a Rust toolchain.

```bash
bun install
bun tauri dev          # desktop
bun tauri android dev  # on a device or emulator
```

Tests:

```bash
bun run build                       # tsc --noEmit + vite build
cd src-tauri && cargo test          # unit + integration, including a full session
```

The integration tests in `src-tauri/src/lib.rs` open a real SQLite database,
seed it, build a day's session, grade it and assert the rotation advances — no
Tauri runtime required.

### Layout

```
src/                       React UI
  lib/api.ts               typed wrappers over the Tauri commands
  lib/useAiStream.ts       accumulates a streamed answer from Tauri events
  components/Markdown.tsx  markdown to React elements, no HTML in between
  screens/Session.tsx      the practice flow (eight exercise shapes)
  screens/Library.tsx      the catalogue: every word, sentence and dialogue
  screens/Songs.tsx        song library, player and the songwriter
  lib/player.ts            the app-wide music player and queue
src-tauri/
  content/                 the curriculum: TSV word lists, TSV sentences, JSON dialogues
  src/db.rs                schema, migration and seeding
  src/session.rs           builds the daily plan
  src/srs.rs               the scheduler
  src/stats.rs             XP, levels, streaks, achievements
  src/ai.rs                OpenRouter chat + speech
  src/songs.rs             songwriter prompt, kie.ai/Suno generation and song downloads
  src/commands.rs          the command surface, including the audio cache
```

## Library

A searchable catalogue of the whole curriculum, whether or not you have met it
yet: all 2007 words and 200 sentences, filterable by topic and searchable in
both languages, plus every dialogue. Each row has its own 🔊.

### Editing the curriculum

The files under `src-tauri/content/` are the source of truth. Word lists are
`polish⇥english⇥danish⇥part-of-speech`; sentences drop the last column;
conversations are JSON with a `speakers` map giving each speaker's gender. After changing them, bump `CONTENT_VERSION` in
`src-tauri/src/db.rs` so existing installs re-seed. Review history is keyed by
row id and survives the re-seed.

`cargo test` fails if two topics define the same headword, if a topic drops
below 190 words, or if a dialogue has a speaker with no gender.

---

## Releases

Every merge to `main` runs `.github/workflows/release.yml`, which tests, builds
a universal macOS `.dmg` and an Android `.apk`, and replaces the rolling
[**latest**](../../releases/tag/latest) release.

### Installing

**Android** — download `Polski-android-universal.apk` from the latest release,
open it on the phone and allow installation from that source.

**macOS** — download `Polski-macos-universal.dmg`. The build is not notarised,
so the first launch needs `xattr -cr /Applications/Polski.app`, or right-click →
Open.

### Android signing

Out of the box CI signs each APK with a throwaway key, which is enough to
install but not to *upgrade* an existing install. To fix that once:

```bash
./scripts/make-android-keystore.sh
```

It creates a keystore and prints the four repository secrets to add
(`ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`,
`ANDROID_KEY_PASSWORD`). Keep the `.jks` file — losing it means future builds
can never upgrade the installed app.
