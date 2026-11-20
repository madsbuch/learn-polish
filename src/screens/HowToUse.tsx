import type { ReactNode } from "react";

import { LessonIcon } from "../components/LessonRow";
import { Avatar, Badge, Card, List, ListItem, Screen, Section, Stack, Text } from "../design";
import { allLessonTypes } from "../lib/lessonTypes";

/** The language taught, and the app's name, as the guide says them. */
const LANGUAGE = "Polish";
const APP = "Polski";
/** How typing is forgiving, in this language. */
const TYPING = "Polish letters are optional when you type: “czesc” counts as “cześć”.";

/** One part of the guide: a heading and a few short paragraphs. */
function Part({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Section title={title}>
      <Card>
        <Stack gap={3}>{children}</Stack>
      </Card>
    </Section>
  );
}

function P({ children }: { children: ReactNode }) {
  return (
    <Text as="p" variant="body">
      {children}
    </Text>
  );
}

/**
 * How to use the app, and why it works the way it does: a page under
 * Settings to read once, and come back to.
 */
export function HowToUse({ onBack }: { onBack?: () => void }) {
  return (
    <Screen title="How to use" subtitle={`${APP}, and the idea behind it`} onBack={onBack} nav={false}>
      <Part title="The idea">
        <P>
          {`You learn ${LANGUAGE} a little every day, from things you care about. ${APP} is built on
          three beliefs:`}
        </P>
        <P>
          <strong>Small and often beats long and rare.</strong> Every lesson does one job and takes minutes. Ten minutes
          a day, kept up, is worth more than an hour on Sunday.
        </P>
        <P>
          <strong>Coming back is how things stick.</strong> A lesson you pass returns later, further apart each time it
          goes well, so you meet it again just before you would forget it.
        </P>
        <P>
          <strong>Your interests are the curriculum.</strong> The tutor builds courses on whatever you want to talk
          about — your work, a trip, football, cooking — and songs and podcasts on the same. Everything can be changed
          or deleted: it is your app.
        </P>
      </Part>

      <Part title="Your day">
        <P>
          Open <strong>Exercises</strong>: <strong>today's lessons</strong> are there — the five that matter most (you
          can change how many in Settings → Practice). A lesson that slipped last time comes first, then the ones whose time has
          come, then up to two new ones — and, when there is room, now and then a mastered lesson whose words are due,
          as a light review (one a day at most; it stays mastered whatever happens).
        </P>
        <P>
          Do them — pass or not, each one you do counts, a quick game too on a day with none to do — and the day is
          done. That's it, nothing piles up. Today's
          lessons are a way in, not the whole day: browse your courses and take what you find interesting — each lists
          its lessons by what matters most. Done days build your streak; every seven days in a row earns a streak freeze
          that covers a missed day by itself. XP is only paid for real work: harder exercises pay more, wrong answers
          and cards you only read pay nothing. A typed answer one letter off, or a sentence written or said with one word
          off (of three or more), is <strong>almost</strong>: half the XP, and it counts as half right. 85% right
          passes a lesson — or, in a short one of up to six cards, all but one. A drill with nothing to score is not
          graded at all.
        </P>
        <P>
          Got a minute? <strong>Quick games</strong> on Home — match pairs against the clock, flip memory cards,
          write down sentences you hear, tell near sounds apart (proszę or prosię?), or say sentences out loud — play
          with what you have already met. They earn a little XP (up to 30 a day), count toward your day, and don&apos;t
          change when a word comes back: that stays the lessons&apos; job. A word counts once a day: answering it right
          five times today doesn&apos;t make it stick — coming back on another day does.
        </P>
        <P>
          Now and then a lesson asks you to <strong>say it</strong>: a sentence or word you know, or your line in a
          dialogue. Tap the mic, speak, and see what was heard, word by word — with a tip when a sound came out as
          another (sz or ż?). Try as often as you like; your best go counts. Somewhere noisy? “Skip speaking for now”
          — or turn speaking off in Settings → Practice. The mic in the tutor&apos;s box writes down what you say, to
          check before sending.
        </P>
      </Part>

      <Part title="Courses and lessons">
        <P>
          A <strong>course</strong> is a theme, and lists its lessons in order. A <strong>lesson</strong> is one thing
          to do, of one of the types below: a <em>drill</em> you tap through, or a <em>conversation</em> with the
          examiner (or, for writing, a partner).
        </P>
        <P>
          Every lesson comes back now and then, so you meet it again before you forget it: that return is its{" "}
          <strong>check-in</strong>. Its page shows when the next one is, and how often.
        </P>
        <P>Each lesson is marked quick, medium or deep, by how long it takes and how deep it goes.</P>
      </Part>

      <Section title="Lesson types">
        <List>
          {allLessonTypes().map((type) => (
            <ListItem
              key={type.id}
              leading={
                <Avatar>
                  <LessonIcon lesson={{ type: type.id, engine: type.engine }} />
                </Avatar>
              }
              title={type.name}
              subtitle={type.description}
              trailing={<Badge tone="sky">{type.engine === "conversation" ? "Conversation" : "Drill"}</Badge>}
            />
          ))}
        </List>
      </Section>

      <Part title="Finishing a course">
        <P>
          A course is <strong>completed</strong> when you have passed each of its core lessons once, and{" "}
          <strong>mastered</strong> when every lesson has full mastery points — challenges too (a challenge is a lesson
          a level up; once the rest is passed, today's lessons offer them), and writing passed once without help.
        </P>
        <P>
          Every pass earns mastery points — 30 for the first, 10 for another the same day, 16 to 35 for one a day to two
          weeks later — and a near miss (60–84% right) half what a pass would; a fail costs 20, except on extra
          practice, which never costs anything. You don&apos;t have to wait for a check-in: practise a lesson as often as
          you like and you get most of the way today — up to 90 points. The last stretch takes a pass on another day,
          any day: mastering is remembering, not cramming. Passes spread out over days are better evidence, so they
          earn more — letting check-ins bring lessons back gets there in fewer passes. Tap a lesson&apos;s mastery on
          its page to see every point and why.
        </P>
        <P>
          <strong>Mastery is for good.</strong> A mastered lesson is never due again and never loses it; its words may
          come back now and then as a light review, so they don&apos;t fade. When every lesson
          of a course is mastered, the course is <strong>ready to close</strong>: tap <em>Close as mastered</em> for
          your celebration, and it moves to <strong>Mastered</strong> — your trophies. Closing is final.
        </P>
        <P>
          Want to come back to a theme? Ask the tutor for a new course on it, a level up — it builds on what the old one
          taught.
        </P>
      </Part>

      <Part title="The tutor">
        <P>
          The <strong>Tutor</strong> tab is where the app grows. Ask in your own words: “a course for ordering in
          restaurants”, “explain when to use this”, “a drill on the numbers”, “a song about my morning”, “what's going
          on in Poland?”.
        </P>
        <P>
          It knows your progress, and remembers what you tell it — goals, what you like, what you find hard — under
          Settings → Memories, where you can read and change it. On any line or word, <em>Talk about it</em> opens a
          short chat about just that.
        </P>
      </Part>

      <Part title="Music">
        <P>
          <strong>Songs</strong> plant a sentence in your head; <strong>podcasts</strong> tell you about a subject in
          short {LANGUAGE} sentences — from Wikipedia, today's news, or a made-up story. Make them yourself on the Music
          tab, or ask the tutor, who can shape them to fit a course.
        </P>
        <P>
          Nothing needs approving: keep what you like and delete the rest. A listening lesson turns any of them into
          practice.
        </P>
        <P>
          In the player&apos;s lyrics, tap a {LANGUAGE} word to see what it means. Turn on <em>Sing along</em> and some
          words go blank just before their line: pick the right one (or type it) before the line ends, and see your
          score when the song is done.
        </P>
      </Part>

      <Part title="Small things that help">
        <P>The speaker plays a line; the turtle plays it slowly.</P>
        <P>{TYPING}</P>
        <P>
          In a conversation, <em>Show translations</em> reveals the English, and the play button reads the whole
          conversation aloud.
        </P>
        <P>
          The AI features need an OpenRouter key (Settings → AI), and songs a kie.ai key (Settings → Songs). The demo course and
          everything already downloaded work without them, offline.
        </P>
      </Part>
    </Screen>
  );
}
