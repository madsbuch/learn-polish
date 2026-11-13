# Polski

A Polish learning app for English speakers. One check-in a day: today's
conversation, the words that belong to it, and its sentences — plus practice
programmes on any theme you like, built for you by the tutor.

Built with Tauri 2 (Rust + SQLite) and React, packaged as a native
**Android** app. (Tauri can also build it for macOS locally; CI no longer does.)

---

## What's in it

| | |
|---|---|
| **2007 words** | Across ten topics, each with an English gloss and a part of speech |
| **200 sentences** | Twenty per topic, with their English translation |
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

### Practising what you choose

Every topic — the ten course topics and your own programmes — has a
**Practise now** button (on the Exercises tab, or under the tutor's reply). That
builds the same kind of session for that topic, with your due reviews mixed in,
without moving the course along.

A topic can also be made the **daily focus**: the daily check-in then serves it
instead of the course until you have met all of its words, and hands back to
the course after that. *Back to the course* on the Today screen ends it early.

### Programmes

A programme is a practice topic of your own: words, sentences and, optionally,
a short dialogue, on any theme — colours, numbers, the words for a trip. They
are stored exactly like the course topics (a `topics` row of kind
`programme`), so the session builder, the scheduler, the library and the
progress screen treat them the same. Programmes are made through the tutor;
they can be deleted from the topic screen.

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

## The tutor (optional)

Add an [OpenRouter](https://openrouter.ai/keys) API key in Settings and pick a
model, and the **Tutor** tab becomes an assistant for the whole app. It speaks
English (it only switches to Polish conversation if you ask it to) and acts in
the app through tools, implemented in `src-tauri/src/tutor.rs`:

| Tool | What it does |
|---|---|
| `create_programme` | Builds a programme on a theme: words, sentences, a dialogue |
| `add_to_programme`, `edit_programme`, `delete_programme` | Changes your programmes (course topics are read-only) |
| `set_daily_focus` | Points the daily check-in at a topic, or back at the course |
| `update_practice_settings` | Daily XP goal, new words per session |
| `list_topics`, `get_topic`, `search_vocabulary`, `get_progress` | Lets it see what you have and how you are doing |
| `suggest_practice` | Puts a *Practise now* button under its reply |
| `make_song`, `list_songs`, `delete_songs` | Has a song written and sung; tidies up songs |

So *"Can you build me a programme for learning the colours?"* produces a
programme you can start from a button under the reply. What the tutor did is
stored with each message, so later turns know the ids of what it made.

The tutor needs a model that supports tool calling; Settings marks those with
🛠 (from OpenRouter's `supported_parameters`). With any model it also explains
words and sentences straight from the exercise you just got wrong.

Answers **stream in** as they are generated, so a long explanation is readable
from the first line rather than after the last, and they are rendered as
markdown — headings, bold, lists and code, not raw `**asterisks**`. The
renderer builds React elements rather than HTML, so model output can never
inject markup and there is no sanitiser to keep in step.

Tool calls are streamed too: the OpenRouter client assembles tool-call
fragments from the event stream, runs each tool against the database and loops
until the model answers in text.

**Every HTTP request is made from Rust**, in `src-tauri/src/ai.rs`, using
`reqwest`. The webview never issues a cross-origin request, so CORS never comes
into it, and the API key never reaches JavaScript — it lives in the app's
private SQLite database and is only ever read inside a command.

**No model is hardcoded.** Model ids go stale, so the app ships without a
default: press *Load models from OpenRouter* in Settings and pick one from the
live catalogue. Until you do, the AI features stay off and say so.

## Songs (optional)

Songs are one of the best ways to get Polish stuck in your head. The **Music**
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
  takes and their cover art are downloaded and stored in the database, and play
  offline, for good. A copy on disk is what the native player and Sonos stream
  from; it is rebuilt from the database whenever it goes missing.
- **Lyrics that follow the music:** when a song is ready the app asks
  kie.ai for Suno's word timings and stores them with each take. The player
  highlights the line being sung, with its English meaning under each Polish
  line; tap a line to jump there. Songs whose timings cannot be fetched (kie.ai
  keeps them for 14 days) show the full lyrics instead.
- **Library and player are separate.** The Music tab is the library: songs
  and playlists, and a page per song with its takes and lyrics. Playing is
  the player's job: a bar above the tabs on every screen, which opens the
  full-screen player with the controls, lyrics that follow the music, and
  what is up next.
- **Playlists** hold takes (the two takes of a song can sound quite
  different): add from a song's page or from the player, reorder, rename.
  Deleting a take takes it out of every playlist.
- **Save or share the MP3.** Every take can be saved wherever you choose
  (the system save dialog) or, on Android, handed to any app through the
  share sheet (a `FileProvider` in the songplayer plugin).
- **One take per song in the queue.** Play and shuffle use each song's
  favourite take (the one played most), and a take played from a song's page
  plays on into the next song — never straight into the other take of the
  same song.
- **Clearing out** is quick: *Select* in the song list deletes many at once,
  and the player can delete the song that is playing and move on.
- **The tutor makes songs too.** Ask it for a song about anything; it briefs
  the songwriter (`make_song`), Suno sings it in the background, and its reply
  links to the song. It can also list and (after asking) delete songs.
- **Built for repetition:** play, shuffle, repeat all / one, play counts, and
  the player bar keeps the music going while you practise.
- **The phone's own player:** on Android, songs play through a native Media3
  service (`src-tauri/plugins/songplayer`), not the web view. That gives the
  lock screen and notification controls, headphone and Bluetooth buttons, a
  pause when headphones are unplugged, and playback with the screen off.
- **Sonos:** *Play on a speaker* finds Sonos rooms and groups on the Wi-Fi
  (or takes an IP address) and moves the queue there. No Sonos account is
  involved: the app serves the downloaded songs over HTTP on the local
  network and loads them into the speaker's queue over UPnP
  (`src-tauri/src/cast.rs`), so Sonos handles next and repeat itself and the
  songs show up in the Sonos app too. While a speaker plays, the phone's lock
  screen, notification and volume keys control the speaker. The phone has to
  stay on the same Wi-Fi, because the speaker streams each song from it.

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

## Your data

Everything the app keeps is in one SQLite database: progress and review
history, programmes, the tutor conversation, every setting (including the
theme and the player's repeat mode), cached speech and the songs themselves.

**Settings → Your data → Export** writes a consistent copy (`VACUUM INTO`) to a
file you choose through the system's save dialog, so it opens in any SQLite
tool. API keys are stripped from the copy (with `secure_delete`, so they are
not left in free pages) unless you choose to include them.

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
seed it, build a day's session, grade it and assert the rotation advances; they
also drive the tutor's tools to build, practise, focus and delete a programme,
and upgrade a database from before the English-only schema — no Tauri runtime
required.

### Layout

```
src/                       React UI
  lib/api.ts               typed wrappers over the Tauri commands
  lib/useAiStream.ts       accumulates a streamed answer from Tauri events
  components/Markdown.tsx  markdown to React elements, no HTML in between
  screens/Session.tsx      the practice flow (eight exercise shapes)
  screens/Exercises.tsx    the home tab: today's check-in, programmes, the course
  screens/Search.tsx       search every word and sentence
  screens/Music.tsx        the music library: songs, playlists, a song's page
  screens/Player.tsx       the full-screen player
  screens/SongComposer.tsx the songwriter
  src/playlists.rs         playlists, and saving or sharing a take's MP3
  lib/player.ts            the app-wide music player and queue
src-tauri/
  content/                 the curriculum: TSV word lists, TSV sentences, JSON dialogues
  src/db.rs                schema, migration and seeding
  src/session.rs           builds the daily plan
  src/srs.rs               the scheduler
  src/stats.rs             XP, levels, streaks, achievements
  src/ai.rs                OpenRouter chat (with streamed tool calls) + speech
  src/tutor.rs             the tutor: prompt, tools and the agent loop
  src/programmes.rs        creating, changing and deleting programmes
  src/songs.rs             songwriter prompt, kie.ai/Suno generation and song downloads
  src/commands.rs          the command surface, including the audio cache
```

## Navigation

Four tabs: **Exercises**, **Tutor**, **Music** and **Settings**.

Exercises is the home screen: today's check-in on top, then your programmes
and the ten course topics, each opening to its dialogue, words and sentences
with *Practise now*. Two screens open from it rather than taking a tab:

- **Progress** — tap the streak or level chip, or *Your progress*: stats,
  the 18-week activity map, per-topic progress and achievements.
- **Search** — the 🔍 button: every word and sentence in the app, whether or
  not you have met it yet, filterable by topic and searchable in Polish or
  English. Each row has its own 🔊.

### Editing the curriculum

The files under `src-tauri/content/` are the source of truth. Word lists are
`polish⇥english⇥part-of-speech`; sentences drop the last column;
conversations are JSON with a `speakers` map giving each speaker's gender. After changing them, bump `CONTENT_VERSION` in
`src-tauri/src/db.rs` so existing installs re-seed. Review history is keyed by
row id and survives the re-seed.

`cargo test` fails if two topics define the same headword, if a topic drops
below 190 words, or if a dialogue has a speaker with no gender.

---

## Releases

Every merge to `main` runs `.github/workflows/release.yml`, which tests, builds
an Android `.apk`, and replaces the rolling
[**latest**](../../releases/tag/latest) release.

### Installing

**Android** — download `Polski-android-universal.apk` from the latest release,
open it on the phone and allow installation from that source.

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
