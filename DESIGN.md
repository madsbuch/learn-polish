# Design system

Polski is built from a small design system in `src/design/`. Every screen is
put together from its components, and every size, colour and spacing comes
from its tokens. That is what keeps a new feature looking like the rest of the
app — follow this page and it will.

```
src/design/tokens.css      every raw value: spacing, type, radii, controls, colours, themes
src/design/base.css        reset, the type scale as classes, layout helpers
src/design/components.css  the components' styles
src/design/index.tsx       the components — import everything from "../design"
src/design/motion.tsx      motion that needs JavaScript (screen direction, count-ups, stagger, confetti)
src/styles.css             feature styles (practice cards, chat, player, lyrics), on tokens only
scripts/check-design.mjs   enforces the mechanical rules; runs in `bun run build`
```

## Principles

1. **One thing matters most per screen.** It gets the only primary button (or
   the hero card). Everything else is secondary, plain, or an icon.
2. **Group, then list.** Related things sit in a `Section` with a short
   uppercase label. Collections are a `List` of `ListItem`s, never hand-made
   rows.
3. **Touch targets are at least 44px.** `Button` and `IconButton` already are;
   don't shrink them below `size="sm"` (36px), and use `sm` only in dense rows.
4. **Booleans are switches.** Never a button that reads "On" or "Off".
5. **Deleting asks once, in one place.** Destructive actions live in a
   "Manage" section at the bottom of the thing's own page, as a
   `ConfirmButton` that says what will be lost. Never in a player, a list row
   or a toolbar.
6. **Every empty list explains itself** with an `EmptyState`: what would be
   here, and the action that fills it.
7. **Explanations hide behind ⓘ.** Screens stay short; `info` on `Screen` or
   `Section` opens the detail in a sheet.
8. **Polish is the content.** Polish words and sentences are `selectable`;
   the sentence to remember is set in `tone="polish"`; English is `dim`.

## Tokens

Use the variables, never the numbers.

| Kind | Tokens |
|---|---|
| Spacing (4px grid) | `--space-1` 4 · `-2` 8 · `-3` 12 · `-4` 16 · `-5` 20 · `-6` 24 · `-7` 32 · `-8` 40; screen edge `--gutter` |
| Type | `caption` 12 · `small` 13.5 · `body` 15 · `lead` 17 · `title` 20 · `headline` 26 · `display` 32 (each `--font-*` with its `--leading-*`) |
| Weight | `--weight-regular` · `-medium` · `-bold` · `-heavy` |
| Radius | `--radius-sm` 8 · `-md` 12 · `-lg` 16 · `-xl` 22 · `-full` |
| Controls | `--control-sm` 36 · `-md` 44 · `-lg` 52; icons `--icon-sm` 18 · `-md` 22 · `-lg` 26 |
| Surfaces | `--bg`, `--surface`, `--surface-raised`, `--surface-sunken`, `--line`, `--line-soft` |
| Text | `--text`, `--text-dim`, `--text-faint`, `--brand-ink`, `--crimson-ink` (Polish) |
| State | `--ok`/`--ok-bg`/`--ok-line`, `--bad`/`--bad-bg`/`--bad-line`, `--info-*` |
| Motion | `--duration-fast` 120ms · `--duration` 200 · `--duration-slow` 320 · `--duration-count` 700 · `--duration-celebrate` 900; `--ease-out`, `--ease-spring`, `--ease-in`; `--stagger-step`, `--enter-shift`, `--rise-shift`; `--hover-shadow`, `--glow-ok`/`-gold`/`-brand` |

Both themes define the same names; a component that uses tokens is correct in
light and dark mode without trying.

## Screens

The layout follows the available window width, including tablet split view.
Below 768px the main tabs sit at the bottom; from 768px they form a left
navigation rail at the window's left edge. From 1000px the practice dashboard
puts today's work in a band across the top and the course library below it
across the full width (course cards are tiles with the picture above the
text from 768px), and a course's or a lesson's page goes into two columns
(`page-split` wrappers: about it on the left, its content on the right). From
1000px Settings keeps its menu on the left and the open sub-screen beside
it (`SplitView`, like Tutor and Music); below that, each sub-screen is pushed
over the tabs. `Screen` defaults to a 760px reading
width; use `width="wide"` for dashboards (capped at `--max-width`). Sheets
become centered dialogs on tablets. Desktop-only additions (hover states) sit
under `(hover: hover) and (pointer: fine)` so touch screens are unchanged.
Keep all scrolling inside the screen or sheet body so headers, composers and
practice controls stay reachable when the keyboard opens. Respect all four
safe-area insets, and allow text and actions to wrap on narrow phones.

Every screen is a `Screen`:

```tsx
<Screen title="Music" info={<p>…</p>} actions={<IconButton … />}>   {/* a tab's root */}
<Screen title="Song" onBack={back}>                                   {/* pushed */}
<Screen title="Now playing" onClose={close} nav={false}>             {/* slid up, covers the tabs */}
```

- A **tab root** has a large title and at most one or two header actions
  (icons, or one plain button such as "Clear").
- A **pushed screen** has a back button and a smaller title. Full screens that
  cover the tab bar pass `nav={false}`.
- The body spaces its children `--space-6` apart. Don't add margins; group
  with `Section` and `Stack` instead.
- An input that must stay in reach (the tutor's message box) goes in
  `footer`: it is docked above the tab bar, never sticky or
  floating. A screen with a footer docks its header too, so only the middle
  scrolls and Back is always in reach.
- Sheets (`Sheet`) are for short tasks that return to where you were: picking
  a playlist, a speaker, reading an explanation.

- The outcome of something the learner just did — "Added to Morning run",
  "Saved to Downloads", a button whose action failed — is a **toast**
  (`toast()` from `src/lib/toast.ts`): it floats at the top of the screen, so
  it is seen however far down the page is. A `Banner` is for a page's own
  state (it could not load, a key is missing), shown where it applies.

## Components

| Component | Use it for |
|---|---|
| `Screen` | Every screen. Header, scrolling, spacing, safe areas. |
| `Section` | A labelled group. `action` takes one plain button ("Select"). |
| `Card` | A block of content or a form. `onClick` makes it a button. |
| `Hero` | The brand gradient. At most one per screen, at the top. |
| `List`, `ListItem` | Any collection. `leading` (cover, avatar, icon), `title`, `subtitle`, `trailing` controls, `onClick` (adds a chevron), `selected`. `stagger` on a list's main collection makes its rows rise in as the screen opens. |
| `Button` | Text actions. `primary` (one per screen/card) · `secondary` (default) · `plain` (low emphasis) · `danger` (deletes) · `inverse` (on the hero) · `success` (continue after a right answer). Sizes `sm`, `md`, `lg`. |
| `IconButton` | Icon-only actions. Always has a `label`. Variants `tinted`, `primary`, `inverse`, `inverse-solid`. `pulse` while what it started goes on (play). |
| `ActionBar`, `Action` | A row of icon-over-label actions (the player). |
| `ConfirmButton` | Anything that deletes. |
| `Switch`, `SettingRow` | Settings: label and description left, control right (`stacked` for sliders and selects). |
| `Field` | A labelled input, select or textarea, with an optional hint. |
| `Segmented` | Two to four views of the same thing. |
| `ChoiceRow`, `Choice` | Filters and presets that scroll sideways. |
| `Badge` | A small status: streak, level, "Failed". |
| `Banner` | A message: `info`, `ok` or `error`. |
| `EmptyState` | What an empty list shows. |
| `Progress`, `StatGrid`, `Stat`, `Spinner`, `Loading` | Feedback. `Progress` fills in as it appears; `memory` makes it move on from where it was last seen. |
| `Typing` | The other side of a chat is writing (three dots). |
| `CountUp`, `Confetti` | A number that counts to its new value (`memory`, `from`); confetti for a day done, a level, a course closed. |
| `Text`, `Stack`, `Inline`, `Avatar` | Type on the scale; layout without margins. |

Icons come from `src/components/icons.tsx` (stroke icons on a 24px grid).
Never a text glyph like `‹`, `✕` or `⌄` in a button.

## Motion

The app moves in one language: things settle in quickly and land softly
(`--ease-out`); what pops — a sheet on a big screen, a right answer, a
switch's knob, a pressed button springing back — overshoots a little
(`--ease-spring`); what leaves accelerates away (`--ease-in`). Durations come
from the tokens: 120–320ms for anything you wait on, longer only for
counting and celebrating.

- **Screens.** `Screen` decides how it comes in: a tab's root rises, a
  pushed screen slides in from the right, a slid-up one (`onClose`) rises
  from below, and whatever is *returned to* comes back from the left. Its own
  back/close buttons mark the way back; anything else that goes back (a
  summary's Done, leaving practice) calls `markBack()` first.
- **Lists.** A screen's main collection rises in row by row (`List stagger`,
  `useStagger`) — only as the screen first shows it, never when it is
  returned to, and never again after: rows a sync adds simply appear.
- **Numbers and bars.** Counters use `CountUp`; bars (`Progress`) and the
  Today ring fill from where they were last seen (`memory`) to the new
  value. Nothing moves on a reload that changed nothing.
- **Practice.** The next card slides in; a right answer swells and glows, a
  wrong one shakes; the XP badge bumps (`useBump`). A day done or a new level
  gets `Confetti`. A sentence tile lifts when picked up and follows the
  finger above the page; the line makes room, and the tile springs into its
  slot (FLIP: one-off, ending untransformed). Memory cards turn edge-on to
  their new face. A running clock writes its own digits (no re-render). A
  checked dictation's words land one after another (an entrance, once).
  A scene (`components/Scene.tsx`) fills the width on a phone and as much
  of the window as the card leaves on a big screen; its rings settle in
  (one-off) and change colour and glow only — a right find glows green, a
  miss shakes softly where it landed, the answer after two misses glows
  gold.
- **Sing along** (the lyrics, `FollowLyrics`): a blank is the word in
  transparent ink on a dashed line, so filling it moves nothing; it fills
  by colour with a glow (a shadow), or shows in gold when missed. The
  choices and the score sit above the scroller, where they may pop; a
  word's meaning opens in a popover inside it, at once, with no fade.
- **Haptics** (phones; `src/lib/haptics.ts`, Settings → Voices & sound): a
  soft tap for a right answer, a firm double buzz for a wrong one, a light
  tick under a primary button or a tile, a roll with every `Confetti`. Not
  tied to reduced motion: it has its own switch.
- **Touch and mouse.** Pressed things scale down a touch and spring back.
  Hover (mouse only, under `(hover: hover) and (pointer: fine)`) changes
  shadow, border or tint — never a transform (see below).
- **Sheets** slide up (a dialog on a big screen springs open) and slide away
  when closed from inside the sheet.

Rules:

1. **Reduced motion.** `base.css` has the one guard for CSS; JavaScript
   motion checks `prefersReducedMotion()` and shows the end state at once.
2. **Compositor only for moves.** Animate `transform` and `opacity`; bars
   slide a full-width fill (`translateX`), never `width`/`height`.
3. **WebKit and scrolling.** An element animating `opacity` or `transform`
   inside a scrolling box gets its own layer in WebKit, which then scrolls a
   moment apart from the rest ("half the box moves, then the other half").
   So: entrances run once, on mount, with `backwards` fill (nothing stays
   transformed); anything continuous or repeated inside a scroller animates
   colours, shadows or a registered custom property (the typing dots, the
   play button's halo, the lyrics' highlight, the Today ring); hover never
   transforms; and never `scroll-behavior: smooth` with per-item transitions.
4. **Cheap.** No animation on a sync's reload, short durations, and one-off
   keyframes rather than running loops (a loop only while something is
   actually happening: thinking, playing).
5. **Tokens.** Durations, easings, shifts and glows come from `tokens.css`.

## Writing

- Sentence case everywhere: "Delete song", not "Delete Song".
- Buttons say what happens: "Practise now", "Save MP3", "Delete".
- Confirmations say what will be lost: "Delete the song and all its takes?"
- Keep descriptions to one line; anything longer goes behind ⓘ.

## Adding to the system

Need something the components can't express? Add it to the design system,
not to the screen:

1. Add any new raw value to `tokens.css`.
2. Add the styles to `components.css` (generic) or `styles.css`
   (particular to one feature), using tokens only.
3. For a generic piece, add a component to `src/design/index.tsx` with a
   one-line comment on when to use it, and list it in the table above.

`bun run build` runs `scripts/check-design.mjs` first. It fails on inline
styles that set spacing, type or colour; on the old class names; on text
glyphs used as icons; and on raw colours or off-scale font sizes outside
`tokens.css`.
