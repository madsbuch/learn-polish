# Governing principles

What Polski is for, and the rules that settle design questions. When a change
is unclear, or two parts of the app disagree, these decide. They come from the
app's owner; change them only on their word.

## The learner

1. **The learner plans their own learning.** Courses are made for what the
   learner needs the language for — a trip to a festival in two days, their
   work, a hobby. The tutor builds what is asked for; the app does not decide
   what is worth learning.

2. **The learner's level never blocks harder content.** A CEFR level describes
   material; it never gates it. Ask for a course at any level, harder or
   easier, and it is made exactly so — never refused, softened or watered
   down. No paternal tutor.

3. **No approvals.** Keeping something is approving it; deleting it is
   rejecting it. Nothing the tutor makes waits for a yes.

4. **No one-time backfills behind the learner's back.** When a new feature
   needs something the learner's existing things don't have yet — a level
   for every course, levels moved out of titles — the app does not fill it
   in by itself at startup (no migration-style upserts or checks of the
   learner's content). It shows a highlighted button that asks the tutor
   ("Add levels to 4 courses"), and the tutor does it in a conversation the
   learner can read; once done, the button goes. New features of this kind
   are built on the tutor. Ongoing work the app already does for the learner
   (finishing a song they asked for, keeping files in place) is fine.

## Progress

5. **A course can always be finished, regardless of time.** Time may help —
   passes spread over days are better evidence and earn more — but it is
   never required. Practising a lesson again today always moves it forward,
   so a learner who wants to can take a lesson most of the way in one go;
   the last stretch to mastered takes a pass on a second day — any day, no
   waiting for a check-in — because mastering is remembering, not cramming.

6. **Mastery cannot be lost.** A mastered lesson stays mastered and is never
   due again. This is part of the game: earned progress is never taken back.
   Its words may still come back now and then as a light review in a spare
   place of the day's lessons, so they don't fade; nothing there can cost it
   its mastery.

7. **Rewards follow learning.** Points measure recall that builds memory:
   XP and mastery points grow with what was answered right, and harder cards
   and harder material pay more. A drill's difficulty is matched to its
   material's level — the course's level is the ceiling, familiarity orders
   the cards within it. Nothing takes progress away for a mistake; a miss
   is answered by the schedule (the lesson comes back sooner), not by a
   penalty.

8. **Closed is closed.** Closing a mastered course is a celebration the
   learner chooses ("Close as mastered", with an animation), and it is final.
   A closed course is a trophy: nothing is added to it, changed in it, or
   brought back from it.

9. **Revisiting a subject is the tutor's job, not the course's.** Forgetting
   happens. When a mastered subject needs another round, the tutor offers a
   new course on it — a level up — rather than reopening the old one.

10. **The daily five is a way in, not the whole day.** It gives a structured
   start: the few lessons that matter most today, then a celebration and
   nothing else. Normally the learner browses courses and takes what they
   find interesting; a course lists its lessons by what matters most.

## Content

11. **One-off lessons live in one place.** A lesson belongs in the course whose
   material it uses — grammar and culture included. One that fits no course
   goes in *Other lessons*, a single folder at the bottom of the course list.
   A lesson is in exactly one course and is never moved: the same practice
   elsewhere is a new lesson there.

12. **Everything made is recreatable.** For each song or podcast, enough is
    kept to rebuild the exact prompt that made it.

## The code

13. **Lesson types are modular.** A lesson type is declared in one place, and
    nothing else branches on it (see `ADDING_A_LESSON_TYPE.md`).

14. **Polski and Dansk are separate codebases.** They share features, not
    code, and may diverge.

15. **Ids are tagged, and shown as titles.** Every row has a TypeID that says
    what it is (`ls_…` a lesson), made on the device that wrote it, so the
    database can sync and an agent's wrong or made-up id fails instead of
    touching the wrong thing. People never read or type them: screens show the
    title instead. See `DATA.md`.
