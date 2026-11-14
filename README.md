# Polski

A Polish learning app for English speakers. Everything is a small block —
a quick drill or a short lesson with a check-in — filed in courses: the
starter courses the app comes with, and courses on any theme you like, built
for you by the tutor.

Built with Tauri 2 (Rust + SQLite) and React, packaged as a native
**Android** app. (Tauri can also build it for macOS locally; CI no longer does.)

---

## What's in it

| | |
|---|---|
| **2007 words** | Across ten starter courses, each with an English gloss and a part of speech |
| **200 sentences** | Twenty per topic, with their English translation |
| **10 conversations** | 207 lines of natural dialogue, one per topic |

The ten starter courses: basics & greetings · family & people · food & restaurant ·
shopping & money · travel & directions · home & daily life · work & study ·
health & body · time, weather & nature · leisure, media & technology.

All of it is compiled into the binary and written once, on a fresh install,
as ordinary courses (plus *Grammar essentials*, holding the starter lessons).
From then on it is your data like anything else: add to it, rename, edit or
delete any course or block — nothing is re-seeded over your changes.

## Blocks

Learning happens in **blocks**: small units with one job each.

- **Drills** are mechanical practice on a handful of items, in one format:
  *words* (new-word cards, multiple choice, typing once known, a memory round),
  *memory* cards, *typing*, *sentences* (build them, then fill gaps), *gaps*,
  or a *dialogue* (read it, then fill its lines).
- **Study blocks** are short lessons — a conjugation, a grammar point, a bit of
  culture — with objectives. Their check-in is an interview: an **examiner**
  agent asks one question at a time (more, and harder ones, the more complex the
  block), gives a line of feedback after each answer, and ends with a verdict:
  pass, partial or fail, plus what to review (`src-tauri/src/examiner.rs`).

- **Listening blocks** tie a study block to a song: play it (the lyrics stay
  hidden unless asked for), then the examiner asks about what you heard —
  what the chorus means, the missing word in a line, a line to translate, and
  for deeper blocks the grammar of what is sung. The song can be replayed
  during the check-in. Every song the tutor makes gets one; any song's page
  has *Listen and answer questions*.

- **Writing blocks** are a situation played out in writing — tell a new
  neighbour where you live, order dinner — with an AI partner who writes and
  understands only Polish. It answers anything else with "Przepraszam, nie
  rozumiem…". The conversation ends, with a verdict and corrections, as soon
  as you have got every goal across (or when you tap End). The partner runs on
  its own model (Settings → Writing partner), so a small, fast one can be used.
  Writing blocks without a course go in *Writing*.

Every block has a **complexity** — low (1–2 min), medium (5–10 min) or deep
(20+ min) — set by how hard its subject is, not by how much is in it. And every
block has its own **check-in period**: the first pass brings it back after the
period, each further pass doubles the gap (up to four months), a partial pass
repeats the period, and a fail brings it back tomorrow. The period can be
changed on the block's page.

Three levels:

- A **course** is a folder on one theme. It lists blocks, in order.
- A **block** is one thing you do, with its own check-in schedule. A drill
  **owns** the words, sentences or dialogue lines it practises; a lesson owns
  its text and objectives.
- A **word** (or sentence) belongs to exactly one block, and a course reaches
  its words only through its blocks.

Giving a course words, sentences or a dialogue cuts them into drills — the
dialogue, words in eights, sentences in fives — skipping anything the course
already teaches. A block can be listed in **several courses** (one block, one
schedule, shown in each); it can be moved between courses, and taking it out
of its last course deletes it, with its words and their history. On a block's
page, *Also in…* and *Move to…* do this by hand; the tutor has tools for it. The starter lessons (`src-tauri/content/lessons/`: być, mieć,
the three present-tense patterns, gender, the seven cases, ty vs. Pan/Pani)
are in *Grammar essentials*, and a lesson the tutor writes without a better
home goes in *Lessons*. A song remembers the course whose words it uses, and
its listening block is filed there; like any block it can be moved or shared
afterwards. The tutor files songs itself: it picks the course a new song fits,
and reads the lyrics of songs left in *Listening* to move them where they
belong. The
**tutor** builds more: courses on any theme, single drills in a chosen format,
and study blocks with their own lessons and objectives.

The **Exercises** tab reads top to bottom: **today** (the XP ring and the next
thing to do), **Due now** (blocks whose check-in has come), **Start next** (the
next unstarted blocks from your newest courses), and **Courses**.

Practising any block counts toward the day's streak.

### Scheduling

Blocks have their own schedule (`src-tauri/src/blocks.rs`). Inside a drill, an
SM-2 style scheduler (`src-tauri/src/srs.rs`) still tracks every word and
sentence, so exercise difficulty follows each item's history: brand-new items
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
| `create_course` | Builds a course on a theme: words, sentences, a dialogue — cut into drills |
| `create_study_block`, `create_drill_block`, `create_listening_block`, `create_writing_block` | Writes a lesson with objectives, one drill in a chosen format, a listening block for a song, or a writing situation with goals |
| `list_blocks`, `get_block`, `update_block`, `delete_block`, `schedule_block`, `suggest_block` | Sees and manages blocks, including the examiner's verdicts |
| `add_to_course`, `edit_course`, `delete_course` | Changes any course, starter ones included |
| `move_block`, `add_block_to_course`, `remove_block_from_course`, `order_course_blocks` | Rearranges blocks between and within courses |
| `update_practice_settings` | Daily XP goal |
| `list_courses`, `get_course`, `search_vocabulary`, `get_progress` | Lets it see what you have and how you are doing |
| `make_song`, `list_songs`, `get_song`, `delete_songs` | Has a song written and sung (filed in the course it fits); reads a song's lyrics to file it; tidies up songs |
| `list_playlists`, `create_playlist`, `add_to_playlist`, `remove_from_playlist`, `rename_playlist`, `delete_playlist` | Manages playlists by song (a song's most-played take goes in) |

You can keep **several conversations**: the Tutor tab lists them, newest
first, each titled after its first question, and each with its own history —
so a grammar question does not drag along last week's course. Rename or
delete one from its ⋯ menu; what the tutor made in the app stays.

So *"Can you build me a programme for learning the colours?"* produces a
course you can open from a button under the reply. What the tutor did is
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

### Memory

The tutor and the examiner share a long-term memory (`memories` table): short
facts about the learner, each a goal, preference, context, strength or
struggle. Both agents see every memory in their system prompt and have
`remember`, `update_memory` and `forget_memory` tools, so what the examiner
notices in a check-in reaches the tutor and the other way round. They are told
to keep memories true — update a struggle once it is overcome rather than add
a contradicting one — and to save lasting patterns, not scores the app already
records. Whatever an agent saves shows up in the chat as a "Remembered: …"
line.

**Settings → Memories** lists them by kind; tap one to edit or forget it, add
your own with **+**, forget everything, or switch memory off (the agents then
neither read nor save memories).

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
- **Length** is a setting (and a choice when composing): about 2, 3 or
  4–5 minutes, the default. Suno has no duration parameter — a song lasts
  as long as its lyrics — so each length is a structure the songwriter
  follows, and long songs also ask Suno for a full-length arrangement.
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
- **The screen stays on** while the full-screen player is open and a song is
  playing, so the lyrics can be followed hands-free (Android sets the
  window's keep-screen-on flag; desktops use the browser's wake lock).
  Pausing or closing the player lets the phone sleep again.
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

## AI costs

**Settings → AI costs** shows what the AI features cost today, over the last
7, 30 or 90 days, and all time — per feature (tutor, check-ins, song lyrics,
song audio, explanations, speech) and day by day with each day's features.

Every paid request is logged in the `ai_costs` table. Chat requests ask
OpenRouter for usage accounting and store the `usage.cost` it reports (also on
streamed answers). Suno songs store the `creditsConsumed` kie.ai reports,
converted at about $0.005 a credit, once per task. OpenRouter's speech endpoint
reports no price, so speech is counted in clips (with the characters spoken)
and shown as unpriced. Costs survive a progress reset.

## Your data

Everything the app keeps is in one SQLite database: progress and review
history, courses, the tutor conversations, every setting (including the
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
also drive the tutor's tools to build, practise and delete a course,
and upgrade a database from before the English-only schema — no Tauri runtime
required.

### Design

The UI is built from a small design system in `src/design/` — tokens for
every size and colour, and components for every screen element. Read
[DESIGN.md](DESIGN.md) before adding a screen; `bun run build` checks the
rules (`bun run check:design` runs the check alone).

### The keyboard on Android

The app draws edge to edge, which stops Android from shrinking it when the
keyboard opens (it would pan the whole screen up instead, sliding the header
away). `src-tauri/android/MainActivity.kt` replaces the activity `tauri android
init` generates — CI copies it in and sets `adjustResize` — and turns the
keyboard's height into bottom padding, so the page lays out above it.
`src/lib/keyboard.ts` then hides the tab bar and player bar while you type.

### Architecture

The webview never touches the network or the database: every read, write and
request is a Tauri command in Rust, and the UI is a typed layer over them
(`src/lib/api.ts`). Rust is organised by part of the app, and **each module
holds its own commands** next to the logic they call (in a `commands`
submodule where the module is otherwise pure); `commands.rs` keeps only what
belongs to the app as a whole.

```
src-tauri/src/
  schema.rs      every table and view as Rust types (Diesel): queries are checked against it
  db.rs          the connection, schema SQL, migrations (old databases are rebuilt in place, ids kept), starter content
  courses.rs     courses: the folders blocks are listed in; cutting items into drills; moving blocks
  blocks.rs      blocks: what they own, their check-in schedule, the home screen's lists
  session.rs     a drill's exercises, by format
  practice.rs    doing a block: checking answers, finishing a drill, the results
  srs.rs         the per-item scheduler inside drills
  stats.rs       XP, levels, streaks, achievements, per-course progress
  agent.rs       the one AI loop: streaming, tool rounds, memory tools, costs
  ai.rs          OpenRouter transport: chat (streamed, with tool calls) and speech
  tutor.rs       the tutor's prompt and tools (each declared with its status line)
  examiner.rs    the check-in interviewer and its verdict
  memories.rs    long-term memory shared by the agents
  costs.rs       the cost center
  speech.rs      Polish read aloud, the voices, the audio cache
  songs.rs       songwriter prompt, kie.ai/Suno generation, downloads, lyric timing
  playlists.rs   playlists, and saving or sharing a take's MP3
  cast.rs        Sonos/UPnP casting
  http.rs        the HTTP clients (OpenRouter, downloads, home network)
  text.rs        shared text helpers
  commands.rs    app-level commands: settings, keys, browsing, explain, reset, export

src/
  design/                  the design system: tokens, components (see DESIGN.md)
  lib/api.ts               typed wrappers over the Tauri commands
  lib/useAiStream.ts       any streamed AI answer: text, tool status, errors, stale streams
  lib/back.ts              Android's back button over in-app pages
  lib/player.ts            the app-wide music player and queue
  screens/                 one file per screen (Exercises, BlockPage, Session, CheckIn, Tutor, Music…)
  components/              pieces shared between screens (Markdown, SetExercises, MiniPlayer…)
```

A few conventions keep it that way:

- **Every query is typed.** Database access goes through
  [Diesel](https://diesel.rs) against `src/schema.rs`, so a misspelt table or
  column, or a value of the wrong type, does not compile. Rows are read into
  `Selectable` structs or typed tuples; inserts and updates use typed column
  assignments. Raw SQL is left only where nothing typed exists: creating the
  schema, PRAGMAs, `VACUUM`, and upgrading old databases whose tables are no
  longer in the schema (and what those read back is still a typed row).
  `schema_matches_the_database` (db.rs) fails if `schema.rs` and the schema
  the app creates drift apart; when adding a column, change both, and
  `diesel print-schema` on a fresh database shows what it should be.
- **Every AI feature goes through `agent::Agent`.** A new agent declares its
  tools with `agent::tool(name, status line, description, schema)` and says
  how to run a call; streaming, costs, memory and errors come with it.
- **Words reach a course only through its blocks.** Query a course's items
  through the `course_words`, `course_sentences` and `course_lines` views.
- **"Topic" in the code is "course" in the app.** The table and ids kept the
  old name (`topics`, `topic_id`); everything the learner sees says course.
- **Pages with a way back use `Screen`'s `onBack`/`onClose` or a `Sheet`**, so
  the system back button works without extra code.

## Navigation

Four tabs: **Exercises**, **Tutor**, **Music** and **Settings**.

Android's back button walks back through the app — closing a sheet first,
then the page you are on, then from another tab to Exercises — and only
leaves the app from Exercises (`src/lib/back.ts`: every page with a ← or ⌄,
and every sheet, adds a history entry that back pops). The player bar above
the tabs can be swiped away to the right, which stops the music.

Exercises is the home screen: today on top, what is due, what to start next,
then your courses. A course page has tabs: **Blocks** (in the course's
order), **Words** and **Sentences** — every item with the drill it belongs to,
one tap away — and *Rename* / *Delete course*. A conversation drill's
dialogue is on that drill's page. Two screens open from it rather than taking a tab:

- **Progress** — tap the streak or level chip, or *Your progress*: stats,
  the 18-week activity map, per-topic progress and achievements.
- **Search** — the 🔍 button: every word and sentence in the app, whether or
  not you have met it yet, filterable by topic and searchable in Polish or
  English. Each row has its own 🔊.

### Editing the curriculum

The files under `src-tauri/content/` are the source of truth. Word lists are
`polish⇥english⇥part-of-speech`; sentences drop the last column;
conversations are JSON with a `speakers` map giving each speaker's gender.
They are the *starter* content: written once on a fresh install
(`seed_starter` in `src-tauri/src/db.rs`) and never re-seeded, since after
that the courses belong to the learner. Databases from before courses were
unified are converted by `unify_courses`: built-in topics and programmes
become courses, loose lessons are filed, and review history is kept.

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
