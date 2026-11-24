# Adding a lesson type

Every lesson has one **type** — words, pictures, scene, story, typing,
memory, sentences, gaps, dialogue, study, listening, writing — and the type says everything
about it. Nothing else in the app branches on a lesson's type: it asks the
type. Adding one means writing it in one place, and giving the screens
anything new it needs.

## The pieces

| Where | What it holds |
|---|---|
| `src-tauri/src/lesson_types/mod.rs` | The `LessonType` struct and the `REGISTRY` of every type. |
| `src-tauri/src/lesson_types/<type>.rs` | One type: its `pub const TYPE: LessonType`, and how it is practised. |
| `src-tauri/src/session.rs` | Building blocks for drills: loading a drill's words and sentences, and one function per exercise (`word_intro`, `recognise`, `picture_options`, …). |
| `src/exercises/` | One module per exercise kind: how it is drawn, what the answer is, whether it is only read. `index.ts` is the registry; a kind without a module does not compile. |
| `src/lib/lessonTypes.ts` | The registry as the screens see it (loaded from the core at startup). |
| `src/components/LessonRow.tsx` | `TYPE_ICONS`: the icon for each icon name. |
| `src/screens/LessonPage.tsx` | `section(name)`: each part a lesson page can show. |

## What a type declares

```rust
pub const TYPE: LessonType = LessonType {
    id: "pictures",                // stored in lessons.lesson_type
    name: "Pictures",              // what the learner sees
    icon: "picture",               // drill | study | write | song | picture | …
    description: "…",              // for the learner ("How to use") and the tutor
    tutor_hint: "…",               // for the tutor only: when to use it (or "")
    run: Run::Exercises(build),    // or Run::Conversation(Conversation { … })
    path: PathGroup::Words,        // where it goes in a course's path
    takes_items: false,            // offered by the tutor's create_drill_lesson
    unaided_to_master: false,      // writing: mastered only once done without help
    sections: &["pictures"],       // what its page shows, in order
    start_label: "Start",
    again_label: "Practise again",
};
```

- **A drill** (`Run::Exercises`) builds its exercises from a `Drill` — the
  lesson, its course, its words and sentences (each marked new or not), and
  decoys for choices — using the building blocks in `session.rs`, or its own.
- **A check-in** (`Run::Conversation`) is a conversation with the examiner.
  `partner: true` makes the other side a partner in a situation (writing);
  `goals_pay` adds XP per goal; `after_verdict` runs after the verdict.

Check-in period, minutes per lesson and mastery points come with every type.

## Steps

1. **Write the type:** `src-tauri/src/lesson_types/<type>.rs` with its `TYPE`
   (and its `build` function for a drill). Add `mod <type>;` and
   `&<type>::TYPE` to `REGISTRY` in `mod.rs`.
2. **New exercises, if any:** a function in `session.rs` (or the type's
   module) that makes the `Exercise`; its XP in `stats::xp_for_answer`; if it
   is only read, its kind in `practice::READING_KINDS`. On the screens: add
   the kind to `ExerciseKind` in `src/lib/schemas.ts`, a module in
   `src/exercises/`, and list it in `src/exercises/index.ts`.
3. **A new icon or page section, if any:** an entry in `TYPE_ICONS`
   (`LessonRow.tsx`), a `case` in `section(name)` (`LessonPage.tsx`).
4. **How it is made:** a drill made from words or sentences needs nothing —
   set `takes_items: true` and the tutor's `create_drill_lesson` offers it.
   One made from something else gets its own creation function in
   `lessons.rs` (or a module of its own) and its own tutor tool (`tutor.rs`),
   as pictures, scenes (`scenes.rs`), stories (`stories.rs`), study, writing
   and listening have.
5. **Test it:** `every_lesson_type_can_be_made_and_practised` (in
   `src-tauri/src/lib.rs`) makes and practises every registered type, checks
   the tutor knows it, and checks that every exercise it produces is one the
   screens know. A new check-in type needs a sample there, or the test says
   so.
6. **Put one in the demo course** (`demo_course.rs`, for new installs only):
   it has a lesson of every type, and its test fails until the new one is
   there. Anything it needs a key for waits, saying so, until one is set.

The tutor learns about the type on its own: its rules list every type in the
registry with its description.
