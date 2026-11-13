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

Both themes define the same names; a component that uses tokens is correct in
light and dark mode without trying.

## Screens

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
  `footer`: it is docked above the tab bar and player bar, never sticky or
  floating. A screen with a footer docks its header too, so only the middle
  scrolls and Back is always in reach.
- Sheets (`Sheet`) are for short tasks that return to where you were: picking
  a playlist, a speaker, reading an explanation.

## Components

| Component | Use it for |
|---|---|
| `Screen` | Every screen. Header, scrolling, spacing, safe areas. |
| `Section` | A labelled group. `action` takes one plain button ("Select"). |
| `Card` | A block of content or a form. `onClick` makes it a button. |
| `Hero` | The brand gradient. At most one per screen, at the top. |
| `List`, `ListItem` | Any collection. `leading` (cover, avatar, icon), `title`, `subtitle`, `trailing` controls, `onClick` (adds a chevron), `selected`. |
| `Button` | Text actions. `primary` (one per screen/card) · `secondary` (default) · `plain` (low emphasis) · `danger` (deletes) · `inverse` (on the hero) · `success` (continue after a right answer). Sizes `sm`, `md`, `lg`. |
| `IconButton` | Icon-only actions. Always has a `label`. Variants `tinted`, `primary`, `inverse`, `inverse-solid`. |
| `ActionBar`, `Action` | A row of icon-over-label actions (the player). |
| `ConfirmButton` | Anything that deletes. |
| `Switch`, `SettingRow` | Settings: label and description left, control right (`stacked` for sliders and selects). |
| `Field` | A labelled input, select or textarea, with an optional hint. |
| `Segmented` | Two to four views of the same thing. |
| `ChoiceRow`, `Choice` | Filters and presets that scroll sideways. |
| `Badge` | A small status: streak, level, "Failed". |
| `Banner` | A message: `info`, `ok` or `error`. |
| `EmptyState` | What an empty list shows. |
| `Progress`, `StatGrid`, `Stat`, `Spinner`, `Loading` | Feedback. |
| `Text`, `Stack`, `Inline`, `Avatar` | Type on the scale; layout without margins. |

Icons come from `src/components/icons.tsx` (stroke icons on a 24px grid).
Never a text glyph like `‹`, `✕` or `⌄` in a button.

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
