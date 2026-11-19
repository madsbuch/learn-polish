import { useEffect, useMemo, useState, type ReactNode } from "react";

import { BarChart, Measure, type Bar } from "../components/Charts";
import { WordChat } from "../components/WordChat";
import {
  Badge,
  Banner,
  Button,
  Card,
  Choice,
  ChoiceRow,
  CountUp,
  EmptyState,
  Field,
  Inline,
  List,
  ListItem,
  Loading,
  Screen,
  Section,
  Segmented,
  Stack,
  Stat,
  StatGrid,
  Text,
  useStagger,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { fileUrl } from "../lib/files";
import {
  ALL_TIME,
  dayLabel,
  filterFor,
  pct,
  PERIODS,
  posGroup,
  setInsightFilter,
  STRENGTHS,
  strengthOf,
  TAG_ASK,
  useInsightFilter,
  whenLabel,
  WORD_STATUS,
} from "../lib/insights";
import { useSyncReload } from "../lib/sync";
import type {
  Activity,
  AttemptView,
  ClassCount,
  Comparison,
  CourseProgress,
  DayPoint,
  Filter,
  HardItem,
  HardKind,
  ItemHistory,
  MetWord,
  MetWordDetail,
  MistakePage,
  Overview,
  SoundRow,
  Strength,
  StructureDetail,
  WordId,
} from "../lib/types";

/*
 * Progress → "What you've met" (words, structures, sounds) and "Insights"
 * (the overview, what is hardest, mistakes by kind, two periods compared,
 * activity and habits). Every page drills down: a kind of mistake → its
 * examples → the item → the item's whole history. They read the same Rust
 * queries the tutor's analytics tools do (analytics.rs), so what the tutor
 * can see, the learner can browse. The period and course chosen are shared
 * by every page (lib/insights.ts).
 */

export type InsightPage =
  | { page: "words" }
  | { page: "word"; wordId: WordId }
  | { page: "structures" }
  | { page: "structure"; key: string }
  | { page: "sounds" }
  | { page: "overview" }
  | { page: "hard"; kind: HardKind }
  | { page: "mistakes"; cls: string | null }
  | { page: "compare" }
  | { page: "activity" }
  | { page: "item"; itemId: string };

interface Nav {
  today: number;
  courses: CourseProgress[];
  open: (page: InsightPage) => void;
  back: () => void;
  onAskTutor: (ask: string) => void;
}

/** The insights pages as a stack of their own over Progress: back pops. */
export function Insights({
  start,
  today,
  courses,
  onDone,
  onAskTutor,
}: {
  start: InsightPage;
  today: number;
  courses: CourseProgress[];
  onDone: () => void;
  onAskTutor: (ask: string) => void;
}) {
  const [stack, setStack] = useState<InsightPage[]>([start]);
  const top = stack[stack.length - 1];
  const nav: Nav = {
    today,
    courses,
    onAskTutor,
    open: (page) => setStack((s) => [...s, page]),
    back: () => (stack.length > 1 ? setStack((s) => s.slice(0, -1)) : onDone()),
  };
  // Keyed by depth, so a page returned to is a fresh screen coming back.
  return <PageFor key={`${stack.length}-${top.page}`} page={top} nav={nav} />;
}

function PageFor({ page, nav }: { page: InsightPage; nav: Nav }) {
  switch (page.page) {
    case "words":
      return <WordsPage nav={nav} />;
    case "word":
      return <WordPage nav={nav} wordId={page.wordId} />;
    case "structures":
      return <StructuresPage nav={nav} />;
    case "structure":
      return <StructurePage nav={nav} tagKey={page.key} />;
    case "sounds":
      return <SoundsPage nav={nav} />;
    case "overview":
      return <OverviewPage nav={nav} />;
    case "hard":
      return <HardPage nav={nav} kind={page.kind} />;
    case "mistakes":
      return <MistakesPage nav={nav} cls={page.cls} />;
    case "compare":
      return <ComparePage nav={nav} />;
    case "activity":
      return <ActivityPage nav={nav} />;
    case "item":
      return <ItemPage nav={nav} itemId={page.itemId} />;
  }
}

// ------------------------------------------------------------ shared pieces

/** Loads what a page shows, again when its inputs change or a sync lands. */
function useLoad<T>(
  load: () => Promise<T>,
  deps: unknown[],
): { data: T | null; error: string | null } {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  useSyncReload(() => setTick((t) => t + 1));
  useEffect(() => {
    let live = true;
    load()
      .then((value) => {
        if (live) {
          setData(value);
          setError(null);
        }
      })
      .catch((cause) => live && setError(errorMessage(cause)));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, tick]);
  return { data, error };
}

/** The shared period and course, as the filter a page asks with. */
function useFilter(nav: Nav): Filter {
  const { period, courseId } = useInsightFilter();
  return useMemo(
    () => filterFor(period, courseId, nav.today),
    [period, courseId, nav.today],
  );
}

/** The period and course choice at the top of a page. */
function Filters({ nav, course = true }: { nav: Nav; course?: boolean }) {
  const { period, courseId } = useInsightFilter();
  return (
    <div className="insight-filters">
      <ChoiceRow>
        {PERIODS.map((p) => (
          <Choice
            key={p.value}
            selected={period === p.value}
            onClick={() => setInsightFilter({ period: p.value })}
          >
            {p.label}
          </Choice>
        ))}
      </ChoiceRow>
      {course ? (
        <select
          aria-label="Course"
          value={courseId ?? ""}
          onChange={(event) =>
            setInsightFilter({
              courseId:
                nav.courses.find((c) => c.course.id === event.target.value)
                  ?.course.id ?? null,
            })
          }
        >
          <option value="">Every course</option>
          {nav.courses.map((c) => (
            <option key={c.course.id} value={c.course.id}>
              {c.course.title}
            </option>
          ))}
        </select>
      ) : null}
    </div>
  );
}

function Page({
  title,
  subtitle,
  info,
  nav,
  children,
}: {
  title: string;
  subtitle?: string;
  info?: ReactNode;
  nav: Nav;
  children: ReactNode;
}) {
  return (
    <Screen
      title={title}
      subtitle={subtitle}
      info={info}
      onBack={nav.back}
      nav={false}
      width="wide"
    >
      {children}
    </Screen>
  );
}

function Wait({ error }: { error: string | null }) {
  return error ? <Banner tone="error">{error}</Banner> : <Loading />;
}

function StrengthBadge({ strength }: { strength: Strength }) {
  const s = strengthOf(strength);
  return <Badge tone={s.tone}>{s.label}</Badge>;
}

/** One answer: what was given against what was expected, its mistake, when. */
function AttemptRow({
  attempt,
  today,
  showItem,
  onOpen,
}: {
  attempt: AttemptView;
  today: number;
  showItem?: boolean;
  onOpen?: () => void;
}) {
  const body = (
    <div className="attempt-row">
      <Inline justify="between" gap={2}>
        <Text variant="caption" tone="faint">
          {attempt.exerciseLabel} · {whenLabel(attempt.at, today)}
        </Text>
        {attempt.mistakeLabel ? (
          <Badge tone={attempt.correct ? "sky" : "crimson"}>
            {attempt.mistakeLabel}
          </Badge>
        ) : attempt.correct ? (
          <Badge tone="mint">Right</Badge>
        ) : null}
      </Inline>
      {showItem && attempt.item ? (
        <Text weight="bold" tone="polish" selectable>
          {attempt.item}
        </Text>
      ) : null}
      <div className="attempt-row__vs">
        <Text variant="small" tone="dim">
          You:
        </Text>
        <span
          className="attempt-given t-small selectable"
          data-correct={attempt.correct}
        >
          {attempt.given || "—"}
        </span>
        {!attempt.correct || attempt.given !== attempt.expected ? (
          <>
            <Text variant="small" tone="dim">
              expected
            </Text>
            <Text variant="small" weight="bold" selectable>
              {attempt.expected}
            </Text>
          </>
        ) : null}
      </div>
      {attempt.words && attempt.words.some((w) => w.s !== "r") ? (
        <div className="attempt-words" aria-label="Word by word">
          {attempt.words.map((w, i) => (
            <span key={i} data-s={w.s} title={WORD_STATUS[w.s] ?? w.s}>
              {w.s === "m" ? `(${w.e})` : w.g || w.e}
            </span>
          ))}
        </div>
      ) : null}
      {attempt.detail &&
      attempt.detail.includes("→") &&
      attempt.mistake !== "unknown_word" &&
      attempt.mistake !== "lookalike" &&
      attempt.mistake !== "typo" ? (
        <Text variant="caption" tone="faint">
          {attempt.detail}
          {attempt.contrast ? ` · ${attempt.contrast}` : ""}
        </Text>
      ) : attempt.contrast ? (
        <Text variant="caption" tone="faint">
          {attempt.contrast}
        </Text>
      ) : null}
    </div>
  );
  return onOpen ? (
    <ListItem title={body} onClick={onOpen} />
  ) : (
    <ListItem title={body} />
  );
}

/** The item behind an answer, when it has a page of its own. */
function itemPage(attempt: {
  itemKind: string;
  itemId: string;
}): InsightPage | null {
  if (attempt.itemId.startsWith("wd_") || attempt.itemId.startsWith("sn_"))
    return { page: "item", itemId: attempt.itemId };
  return null;
}

function Attempts({
  attempts,
  nav,
  showItem,
  linked = true,
  empty,
}: {
  attempts: AttemptView[];
  nav: Nav;
  showItem?: boolean;
  /** Each answer opens its item (not on the item's own page). */
  linked?: boolean;
  empty: string;
}) {
  if (attempts.length === 0) {
    return (
      <Card>
        <Text variant="small" tone="dim">
          {empty}
        </Text>
      </Card>
    );
  }
  return (
    <List>
      {attempts.map((a) => {
        const target = linked ? itemPage(a) : null;
        return (
          <AttemptRow
            key={a.id}
            attempt={a}
            today={nav.today}
            showItem={showItem}
            onOpen={target ? () => nav.open(target) : undefined}
          />
        );
      })}
    </List>
  );
}

function MistakeMeasures({
  counts,
  nav,
  memory,
}: {
  counts: ClassCount[];
  nav: Nav;
  memory: string;
}) {
  const max = Math.max(1, ...counts.map((c) => c.count));
  if (counts.length === 0) {
    return (
      <Text variant="small" tone="dim">
        No mistakes here yet.
      </Text>
    );
  }
  return (
    <Stack gap={1}>
      {counts.map((c) => (
        <Measure
          key={c.class}
          label={c.label}
          value={c.count}
          max={max}
          memory={`${memory}:${c.class}`}
          onClick={() => nav.open({ page: "mistakes", cls: c.class })}
        />
      ))}
    </Stack>
  );
}

/** Accuracy per day (or week) as bars, each day of the span shown. */
function accuracyBars(
  points: DayPoint[],
  from: number,
  to: number,
  step: number,
  today: number,
): Bar[] {
  const by = new Map(points.map((p) => [p.day, p]));
  const bars: Bar[] = [];
  for (let day = from; day <= to; day += step) {
    let attempts = 0;
    let correct = 0;
    for (let d = day; d < day + step; d++) {
      const p = by.get(d);
      attempts += p?.attempts ?? 0;
      correct += p?.correct ?? 0;
    }
    const share = attempts ? correct / attempts : 0;
    bars.push({
      key: day,
      label: `${step === 7 ? "Week of " : ""}${dayLabel(day, today)}: ${attempts ? `${pct(share)} of ${attempts}` : "no answers"}`,
      value: attempts ? Math.round(share * 100) : 0,
      share,
      empty: attempts === 0,
      faint: attempts > 0 && attempts < 3,
    });
  }
  return bars;
}

// ------------------------------------------------------------ What you've met: words

type Sort = "tricky" | "az" | "recent";

function WordsPage({ nav }: { nav: Nav }) {
  const { data, error } = useLoad(() => api.metWords(), []);
  const [query, setQuery] = useState("");
  const [strength, setStrength] = useState<Strength | "all">("all");
  const [course, setCourse] = useState("");
  const [pos, setPos] = useState("");
  const [sort, setSort] = useState<Sort>("tricky");
  const rise = useStagger(data !== null);
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = (data ?? []).filter(
      (w) =>
        (strength === "all" || w.strength === strength) &&
        (!course || w.courses.some((c) => c.id === course)) &&
        (!pos || posGroup(w.pos) === pos) &&
        (!q ||
          w.pl.toLowerCase().includes(q) ||
          w.en.toLowerCase().includes(q)),
    );
    if (sort === "az")
      return [...list].sort((a, b) => a.pl.localeCompare(b.pl, "pl"));
    if (sort === "recent")
      return [...list].sort((a, b) => (b.lastAt ?? 0) - (a.lastAt ?? 0));
    return list;
  }, [data, query, strength, course, pos, sort]);
  const groups = useMemo(
    () => [...new Set((data ?? []).map((w) => posGroup(w.pos)))].sort(),
    [data],
  );
  const counts = (s: Strength) =>
    (data ?? []).filter((w) => w.strength === s).length;
  return (
    <Page
      title="Words you've met"
      subtitle={data ? `${data.length} words` : undefined}
      nav={nav}
      info={<StrengthInfo />}
    >
      {!data ? (
        <Wait error={error} />
      ) : data.length === 0 ? (
        <EmptyState icon="🔤" title="No words met yet">
          Words show up here once you practise them in a lesson.
        </EmptyState>
      ) : (
        <>
          <Stack gap={3}>
            <Field label="Search">
              <input
                value={query}
                placeholder="Polish or English"
                onChange={(event) => setQuery(event.target.value)}
              />
            </Field>
            <ChoiceRow>
              <Choice
                selected={strength === "all"}
                onClick={() => setStrength("all")}
              >
                All
              </Choice>
              {STRENGTHS.map((s) => (
                <Choice
                  key={s.value}
                  selected={strength === s.value}
                  onClick={() => setStrength(s.value)}
                >
                  {s.label} · {counts(s.value)}
                </Choice>
              ))}
            </ChoiceRow>
            <Inline gap={2} wrap>
              <select
                aria-label="Course"
                value={course}
                onChange={(event) => setCourse(event.target.value)}
              >
                <option value="">Every course</option>
                {nav.courses.map((c) => (
                  <option key={c.course.id} value={c.course.id}>
                    {c.course.title}
                  </option>
                ))}
              </select>
              <select
                aria-label="Part of speech"
                value={pos}
                onChange={(event) => setPos(event.target.value)}
              >
                <option value="">Every kind of word</option>
                {groups.map((g) => (
                  <option key={g} value={g}>
                    {g}
                  </option>
                ))}
              </select>
            </Inline>
            <div className="insight-segmented">
              <Segmented
                value={sort}
                onChange={setSort}
                options={[
                  { value: "tricky", label: "Trickiest" },
                  { value: "az", label: "A–Z" },
                  { value: "recent", label: "Recent" },
                ]}
              />
            </div>
          </Stack>
          {shown.length === 0 ? (
            <EmptyState title="No words match">Try another filter.</EmptyState>
          ) : (
            <div className={rise ? `list ${rise}` : "list"}>
              {shown.slice(0, 400).map((w) => (
                <WordRow
                  key={w.id}
                  word={w}
                  onOpen={() => nav.open({ page: "word", wordId: w.id })}
                />
              ))}
            </div>
          )}
        </>
      )}
    </Page>
  );
}

function StrengthInfo() {
  return (
    <p>
      Tricky: you missed at least a third of your last answers, or the last two.
      Solid: three weeks between reviews, or your last four answers right.
      Learning: everything in between. New: met, not yet answered.
    </p>
  );
}

function WordRow({ word, onOpen }: { word: MetWord; onOpen: () => void }) {
  return (
    <ListItem
      leading={
        word.image ? (
          <img
            className="met-thumb"
            src={fileUrl(word.image)}
            alt=""
            loading="lazy"
          />
        ) : undefined
      }
      title={
        <Text tone="polish" weight="bold">
          {word.pl}
        </Text>
      }
      subtitle={`${word.en}${word.attempts ? ` · ${word.attempts - word.wrong} of ${word.attempts} right` : ""}`}
      trailing={<StrengthBadge strength={word.strength} />}
      chevron
      onClick={onOpen}
    />
  );
}

function WordPage({ nav, wordId }: { nav: Nav; wordId: WordId }) {
  const { data, error } = useLoad(() => api.metWord(wordId), [wordId]);
  const [talking, setTalking] = useState(false);
  if (!data) {
    return (
      <Page title="Word" nav={nav}>
        <Wait error={error} />
      </Page>
    );
  }
  return (
    <Page title={data.word.pl} subtitle={data.word.en} nav={nav}>
      <div className="insight-board">
        <div className="insight-col">
          <WordHead detail={data} onTalk={() => setTalking(true)} />
          <Section title="Your answers">
            <Attempts
              attempts={data.history.history}
              nav={nav}
              linked={false}
              empty="No answers yet: it was only seen."
            />
          </Section>
        </div>
        <div className="insight-col">
          <Section title="Typical mistakes">
            <Card>
              <MistakeMeasures
                counts={data.history.mistakes}
                nav={nav}
                memory={`word:${wordId}`}
              />
            </Card>
          </Section>
          {data.examples.length ? (
            <Section title="Examples">
              <List>
                {data.examples.map((e) => (
                  <ListItem
                    key={e.pl}
                    title={<Text tone="polish">{e.pl}</Text>}
                    subtitle={e.en}
                    selectable
                  />
                ))}
              </List>
            </Section>
          ) : null}
          <Section title="Lessons">
            <List>
              {data.lessons.map((l) => (
                <ListItem key={l.id} title={l.title} />
              ))}
            </List>
          </Section>
          {data.history.grammar.length ? (
            <Section title="Grammar">
              <Inline gap={2} wrap>
                {data.history.grammar.map((t) => (
                  <Badge
                    key={t.key}
                    tone="brand"
                    onClick={() => nav.open({ page: "structure", key: t.key })}
                  >
                    {t.name}
                  </Badge>
                ))}
              </Inline>
            </Section>
          ) : null}
          <Events history={data.history} today={nav.today} />
        </div>
      </div>
      {talking ? (
        <WordChat
          itemKind="word"
          itemId={wordId}
          title={data.word.pl}
          onClose={() => setTalking(false)}
        />
      ) : null}
    </Page>
  );
}

function WordHead({
  detail,
  onTalk,
}: {
  detail: MetWordDetail;
  onTalk: () => void;
}) {
  const w = detail.word;
  return (
    <Card>
      <Stack gap={3}>
        {w.image ? (
          <img className="met-picture" src={fileUrl(w.image)} alt={w.en} />
        ) : null}
        <Inline justify="between" gap={2}>
          <Stack gap={1}>
            <Text variant="display" tone="polish" selectable>
              {w.pl}
            </Text>
            <Text tone="dim" selectable>
              {w.en}
            </Text>
          </Stack>
          <StrengthBadge strength={w.strength} />
        </Inline>
        <StatGrid>
          <Stat
            value={<CountUp value={w.attempts} from={0} />}
            label="answers"
          />
          <Stat
            value={<CountUp value={w.attempts - w.wrong} from={0} />}
            label="right"
          />
          <Stat value={<CountUp value={w.wrong} from={0} />} label="missed" />
        </StatGrid>
        {detail.near ? (
          <Text variant="small" tone="dim">
            Sounds like <strong>{detail.near.pl}</strong> ({detail.near.en})
          </Text>
        ) : null}
        <Button icon={<span aria-hidden="true">✨</span>} onClick={onTalk}>
          Ask the tutor about this word
        </Button>
      </Stack>
    </Card>
  );
}

function Events({ history, today }: { history: ItemHistory; today: number }) {
  if (history.events.length === 0) return null;
  return (
    <Section title="Other times you met it">
      <List>
        {history.events.slice(0, 30).map((e, i) => (
          <ListItem
            key={`${e.at}-${i}`}
            title={e.label}
            subtitle={[
              whenLabel(e.at, today),
              e.details.source,
              e.details.reason,
            ]
              .filter(Boolean)
              .join(" · ")}
          />
        ))}
      </List>
    </Section>
  );
}

// ------------------------------------------------------------ structures

function StructuresPage({ nav }: { nav: Nav }) {
  const filter = useFilter(nav);
  const { data, error } = useLoad(() => api.metStructures(filter), [filter]);
  const rise = useStagger(data !== null);
  return (
    <Page
      title="Structures"
      subtitle="The grammar you've met, trickiest first"
      nav={nav}
      info={
        <p>
          Cases, verb forms, agreement and more, from the grammar tags on your
          words and sentences. Tap one for its explanation, your examples and
          your mistakes.
        </p>
      }
    >
      <Filters nav={nav} />
      {!data ? (
        <Wait error={error} />
      ) : (
        <>
          {data.untagged > 0 ? (
            <Card>
              <Stack gap={3}>
                <Text variant="small">
                  {data.untagged === 1
                    ? "1 word or sentence has"
                    : `${data.untagged} words and sentences have`}{" "}
                  no grammar tags yet, so it isn't counted here.
                </Text>
                <Button
                  variant="primary"
                  onClick={() => nav.onAskTutor(TAG_ASK)}
                >
                  ✨ Tag the grammar in your courses
                </Button>
              </Stack>
            </Card>
          ) : null}
          {data.tags.length === 0 ? (
            <EmptyState icon="🧩" title="No structures yet">
              New lessons come with their grammar tagged; older ones once the
              tutor has tagged them.
            </EmptyState>
          ) : (
            <div className={rise ? `list ${rise}` : "list"}>
              {data.tags.map((row) => (
                <ListItem
                  key={row.tag.key}
                  title={row.tag.name}
                  subtitle={`${row.items} item${row.items === 1 ? "" : "s"}${row.accuracy === null ? " · not answered yet" : ` · ${pct(row.accuracy)} right of ${row.attempts}`}`}
                  trailing={
                    row.accuracy === null ? (
                      <Badge>New</Badge>
                    ) : (
                      <Badge
                        tone={
                          row.accuracy < 0.7
                            ? "crimson"
                            : row.accuracy < 0.9
                              ? "gold"
                              : "mint"
                        }
                      >
                        {pct(row.accuracy)}
                      </Badge>
                    )
                  }
                  chevron
                  onClick={() =>
                    nav.open({ page: "structure", key: row.tag.key })
                  }
                />
              ))}
            </div>
          )}
        </>
      )}
    </Page>
  );
}

function StructurePage({ nav, tagKey }: { nav: Nav; tagKey: string }) {
  const filter = useFilter(nav);
  const { data, error } = useLoad(
    () => api.metStructure(tagKey, filter),
    [tagKey, filter],
  );
  if (!data) {
    return (
      <Page title="Structure" nav={nav}>
        <Wait error={error} />
      </Page>
    );
  }
  return <StructureBody data={data} nav={nav} />;
}

function StructureBody({ data, nav }: { data: StructureDetail; nav: Nav }) {
  const { row } = data;
  const weeks = row.weeks;
  const from = weeks.length ? weeks[0].day : nav.today;
  const bars = weeks.length
    ? accuracyBars(weeks, from, weeks[weeks.length - 1].day, 7, nav.today)
    : [];
  return (
    <Page title={row.tag.name} subtitle={row.tag.group} nav={nav}>
      <Filters nav={nav} />
      <div className="insight-board">
        <div className="insight-col">
          <Card>
            <Stack gap={3}>
              <Text selectable>{row.tag.explain}</Text>
              <StatGrid>
                <Stat
                  value={<CountUp value={row.items} from={0} />}
                  label="items"
                />
                <Stat
                  value={<CountUp value={row.attempts} from={0} />}
                  label="answers"
                />
                <Stat
                  value={
                    row.accuracy === null ? (
                      "—"
                    ) : (
                      <CountUp
                        value={Math.round(row.accuracy * 100)}
                        from={0}
                        format={(v) => `${v}%`}
                      />
                    )
                  }
                  label="right"
                />
              </StatGrid>
            </Stack>
          </Card>
          <Section title="Right answers, week by week">
            <Card>
              {bars.length ? (
                <BarChart
                  bars={bars}
                  top="100%"
                  start={bars[0]?.label.split(":")[0]}
                  end="This week"
                  label={`Share right each week for ${row.tag.name}`}
                />
              ) : (
                <Text variant="small" tone="dim">
                  Not answered yet.
                </Text>
              )}
            </Card>
          </Section>
          <Section title="Mistakes">
            <Attempts
              attempts={data.mistakes}
              nav={nav}
              showItem
              empty="No mistakes on it yet."
            />
          </Section>
        </div>
        <div className="insight-col">
          <Section title="Kinds of mistake">
            <Card>
              <MistakeMeasures
                counts={data.classes}
                nav={nav}
                memory={`structure:${row.tag.key}`}
              />
            </Card>
          </Section>
          <Section title="Your examples">
            <List>
              {data.examples.map((e) => (
                <ListItem
                  key={e.itemId}
                  title={<Text tone="polish">{e.pl}</Text>}
                  subtitle={`${e.en}${e.attempts ? ` · missed ${e.wrong} of ${e.attempts}` : ""}`}
                  onClick={() => nav.open({ page: "item", itemId: e.itemId })}
                />
              ))}
            </List>
          </Section>
        </div>
      </div>
    </Page>
  );
}

// ------------------------------------------------------------ sounds

function SoundsPage({ nav }: { nav: Nav }) {
  const filter = useFilter(nav);
  const { data, error } = useLoad(() => api.metSounds(filter), [filter]);
  return (
    <Page
      title="Sounds"
      subtitle="Near pairs from sound pairs and speaking"
      nav={nav}
      info={
        <p>
          Sounds an English ear runs together. Hits come from sound-pair cards;
          misses also from speaking, when a word was heard as its neighbour.
        </p>
      }
    >
      <Filters nav={nav} course={false} />
      {!data ? (
        <Wait error={error} />
      ) : data.length === 0 ? (
        <EmptyState icon="👂" title="No sounds practised yet">
          Words lessons end with a few sound pairs; the quick game Sound pairs
          plays them all.
        </EmptyState>
      ) : (
        <div className="insight-board">
          {[0, 1].map((col) => (
            <div className="insight-col" key={col}>
              {data
                .filter((_, i) => i % 2 === col)
                .map((s) => (
                  <SoundCard key={s.label} sound={s} nav={nav} />
                ))}
            </div>
          ))}
        </div>
      )}
    </Page>
  );
}

function SoundCard({ sound, nav }: { sound: SoundRow; nav: Nav }) {
  return (
    <Section title={sound.label}>
      <Card>
        <Stack gap={3}>
          <Measure
            label="Heard right"
            value={sound.hits}
            max={sound.attempts}
            shown={`${sound.hits} of ${sound.attempts} · ${pct(sound.accuracy)}`}
            memory={`sound:${sound.label}`}
            tone="mint"
          />
          {sound.sides.map((side) => (
            <Text key={side.said} variant="small" tone="dim">
              When it was <strong>{side.said}</strong>: missed {side.missed} of{" "}
              {side.attempts}
              {side.heardAs ? (
                <>
                  {" "}
                  — you heard <strong>{side.heardAs}</strong>
                </>
              ) : null}
            </Text>
          ))}
        </Stack>
      </Card>
      {sound.examples.length ? (
        <Attempts attempts={sound.examples.slice(0, 4)} nav={nav} empty="" />
      ) : null}
    </Section>
  );
}

// ------------------------------------------------------------ insights: overview

function OverviewPage({ nav }: { nav: Nav }) {
  const filter = useFilter(nav);
  const { data, error } = useLoad(
    () => api.analyticsOverview(filter),
    [filter],
  );
  return (
    <Page title="Overview" subtitle="How your answers are going" nav={nav}>
      <Filters nav={nav} />
      {!data ? <Wait error={error} /> : <OverviewBody data={data} nav={nav} />}
    </Page>
  );
}

const TREND: Record<Overview["trend"], string> = {
  improving: "Improving",
  slipping: "Slipping a little",
  steady: "Steady",
  new: "Just started",
};

function OverviewBody({ data, nav }: { data: Overview; nav: Nav }) {
  const from = data.filter.fromDay ?? data.daily[0]?.day ?? nav.today;
  const span = nav.today - from + 1;
  const step = span > 120 ? 7 : 1;
  const first = step === 7 ? from - ((from + 3) % 7) : from;
  const bars = accuracyBars(data.daily, first, nav.today, step, nav.today);
  if (data.attempts === 0) {
    return (
      <EmptyState icon="📈" title="No answers in this period">
        Practise a lesson and your answers show up here.
      </EmptyState>
    );
  }
  return (
    <div className="insight-board">
      <div className="insight-col">
        <StatGrid>
          <Stat
            value={<CountUp value={data.attempts} memory="insights:answers" />}
            label="answers"
          />
          <Stat
            value={
              <CountUp
                value={Math.round(data.accuracy * 100)}
                memory="insights:accuracy"
                format={(v) => `${v}%`}
              />
            }
            label="right"
          />
          <Stat
            value={<CountUp value={data.daysActive} memory="insights:days" />}
            label="days practised"
          />
          <Stat
            value={<CountUp value={data.minutes} memory="insights:minutes" />}
            label="minutes"
          />
        </StatGrid>
        <Card>
          <Stack gap={2}>
            <Inline justify="between">
              <Text weight="bold">{TREND[data.trend]}</Text>
              {data.accuracyBefore !== null ? (
                <Text variant="small" tone="dim">
                  {pct(data.accuracyBefore)} → {pct(data.accuracy)}
                </Text>
              ) : null}
            </Inline>
            <Text variant="small" tone="dim">
              Against the same length of time just before.{" "}
              {data.lettersForgiven
                ? `${data.lettersForgiven} right answers had Polish letters left off.`
                : ""}
            </Text>
          </Stack>
        </Card>
        <Section
          title={
            step === 7
              ? "Right answers, week by week"
              : "Right answers, day by day"
          }
          info={
            <p>
              Each bar is the share of answers you got right that day; a pale
              bar had fewer than three answers.
            </p>
          }
        >
          <Card>
            <BarChart
              bars={bars}
              top="100%"
              start={dayLabel(first, nav.today)}
              end="Today"
              label="Share of answers right over time"
            />
          </Card>
        </Section>
        <Section
          title="Kinds of mistake"
          info={
            <p>
              Every wrong answer gets a kind, worked out when it is checked. Tap
              one for the examples.
            </p>
          }
        >
          <Card>
            <MistakeMeasures
              counts={data.mistakes}
              nav={nav}
              memory="insights:classes"
            />
          </Card>
        </Section>
      </div>
      <div className="insight-col">
        <Section title="Look closer">
          <List>
            <ListItem
              leading={<span aria-hidden="true">🎯</span>}
              title="Hardest"
              subtitle="Words, sentences, structures, sounds"
              onClick={() => nav.open({ page: "hard", kind: "word" })}
            />
            <ListItem
              leading={<span aria-hidden="true">🔎</span>}
              title="Mistakes by kind"
              onClick={() => nav.open({ page: "mistakes", cls: null })}
            />
            <ListItem
              leading={<span aria-hidden="true">⚖️</span>}
              title="Compare two periods"
              onClick={() => nav.open({ page: "compare" })}
            />
            <ListItem
              leading={<span aria-hidden="true">🕰️</span>}
              title="Activity and habits"
              onClick={() => nav.open({ page: "activity" })}
            />
          </List>
        </Section>
        <AreaList title="Weakest" areas={data.weakest} nav={nav} />
        <AreaList title="Strongest" areas={data.strongest} nav={nav} />
      </div>
    </div>
  );
}

function AreaList({
  title,
  areas,
  nav,
}: {
  title: string;
  areas: Overview["weakest"];
  nav: Nav;
}) {
  if (areas.length === 0) return null;
  return (
    <Section title={title}>
      <List>
        {areas.map((a) => (
          <ListItem
            key={`${a.kind}-${a.key}`}
            title={a.name}
            subtitle={`${a.kind === "structure" ? "Grammar" : a.kind === "sound" ? "Sound" : "Exercise"} · ${a.attempts} answers`}
            trailing={
              <Badge
                tone={
                  a.accuracy < 0.7
                    ? "crimson"
                    : a.accuracy < 0.9
                      ? "gold"
                      : "mint"
                }
              >
                {pct(a.accuracy)}
              </Badge>
            }
            chevron={a.kind !== "exercise"}
            onClick={
              a.kind === "structure"
                ? () => nav.open({ page: "structure", key: a.key })
                : a.kind === "sound"
                  ? () => nav.open({ page: "sounds" })
                  : undefined
            }
          />
        ))}
      </List>
    </Section>
  );
}

// ------------------------------------------------------------ hardest

const HARD_LABELS: { value: HardKind; label: string }[] = [
  { value: "word", label: "Words" },
  { value: "sentence", label: "Sentences" },
  { value: "structure", label: "Structures" },
  { value: "sound", label: "Sounds" },
];

function HardPage({ nav, kind: first }: { nav: Nav; kind: HardKind }) {
  const [kind, setKind] = useState<HardKind>(first);
  const filter = useFilter(nav);
  const { data, error } = useLoad(
    () => api.analyticsHard(kind, filter, 40),
    [kind, filter],
  );
  const rise = useStagger(data !== null);
  return (
    <Page
      title="Hardest"
      subtitle="Ranked by recent misses"
      nav={nav}
      info={
        <p>
          Ranked by how often you miss it, the latest answers counting most. Tap
          one for every answer you gave.
        </p>
      }
    >
      <Filters nav={nav} />
      <div className="insight-segmented">
        <Segmented value={kind} onChange={setKind} options={HARD_LABELS} />
      </div>
      {!data ? (
        <Wait error={error} />
      ) : data.length === 0 ? (
        <EmptyState icon="🌱" title="Nothing missed in this period">
          Well done — or try a longer period.
        </EmptyState>
      ) : (
        <div className={rise ? `list ${rise}` : "list"}>
          {data.map((h) => (
            <HardRow key={`${h.kind}-${h.key}`} item={h} nav={nav} />
          ))}
        </div>
      )}
    </Page>
  );
}

function HardRow({ item, nav }: { item: HardItem; nav: Nav }) {
  const open =
    item.kind === "structure"
      ? () => nav.open({ page: "structure", key: item.key })
      : item.kind === "sound"
        ? () => nav.open({ page: "sounds" })
        : () => nav.open({ page: "item", itemId: item.key });
  const last = item.last[0];
  return (
    <ListItem
      title={
        item.kind === "word" || item.kind === "sentence" ? (
          <Text tone="polish" weight="bold">
            {item.title}
          </Text>
        ) : (
          item.title
        )
      }
      subtitle={
        <>
          {`Missed ${item.wrong} of ${item.attempts}`}
          {item.mistakes[0]
            ? ` · mostly ${item.mistakes[0].label.toLowerCase()}`
            : ""}
          {last ? ` · last: “${last.given || "—"}” for “${last.expected}”` : ""}
        </>
      }
      trailing={
        <Badge tone={item.accuracy < 0.5 ? "crimson" : "gold"}>
          {pct(item.accuracy)}
        </Badge>
      }
      chevron
      onClick={open}
    />
  );
}

// ------------------------------------------------------------ mistakes

function MistakesPage({ nav, cls }: { nav: Nav; cls: string | null }) {
  const filter = useFilter(nav);
  const overview = useLoad(
    () =>
      cls === null ? api.analyticsOverview(filter) : Promise.resolve(null),
    [cls, filter],
  );
  const page = useLoad(
    () =>
      cls === null ? Promise.resolve(null) : api.analyticsMistakes(cls, filter),
    [cls, filter],
  );
  if (cls === null) {
    return (
      <Page title="Mistakes by kind" nav={nav} info={<MistakeInfo />}>
        <Filters nav={nav} />
        {!overview.data ? (
          <Wait error={overview.error} />
        ) : (
          <Card>
            <MistakeMeasures
              counts={overview.data.mistakes}
              nav={nav}
              memory="mistakes:classes"
            />
          </Card>
        )}
      </Page>
    );
  }
  return (
    <Page
      title={page.data?.label ?? "Mistakes"}
      subtitle={
        page.data
          ? `${page.data.count} in this period · ${pct(page.data.share)} of all mistakes`
          : undefined
      }
      nav={nav}
      info={<MistakeInfo />}
    >
      <Filters nav={nav} />
      {!page.data ? (
        <Wait error={page.error} />
      ) : (
        <MistakeBody data={page.data} nav={nav} />
      )}
    </Page>
  );
}

function MistakeInfo() {
  return (
    <p>
      Polish letters: right but for ą, ę, ł, ś…. Endings: the right word in the
      wrong form. Word order: the right words, shuffled. Sounds: heard as its
      near neighbour. Prepositions: a place word taken for another. Typos: a
      letter or two off. Look-alike: a similar option picked. Some words wrong:
      part of a sentence. Didn't know it: no resemblance.
    </p>
  );
}

function MistakeBody({ data, nav }: { data: MistakePage; nav: Nav }) {
  if (data.count === 0) {
    return (
      <EmptyState title="None in this period">Try a longer period.</EmptyState>
    );
  }
  const max = Math.max(1, ...data.items.map((i) => i.count));
  return (
    <div className="insight-board">
      <div className="insight-col">
        <Section title="Examples">
          <Attempts attempts={data.examples} nav={nav} showItem empty="" />
        </Section>
      </div>
      <div className="insight-col">
        <Section title="Most often on">
          <Card>
            <Stack gap={1}>
              {data.items.map((i) => {
                const target = itemPage(i);
                return (
                  <Measure
                    key={i.itemId || i.title}
                    label={i.title}
                    value={i.count}
                    max={max}
                    onClick={target ? () => nav.open(target) : undefined}
                  />
                );
              })}
            </Stack>
          </Card>
        </Section>
      </div>
    </div>
  );
}

// ------------------------------------------------------------ compare

type Preset = "weeks" | "fortnights" | "months";

function presetFilters(
  preset: Preset,
  today: number,
  courseId: Filter["courseId"],
): [Filter, Filter, string, string] {
  const len = preset === "weeks" ? 7 : preset === "fortnights" ? 14 : 30;
  const names: Record<Preset, [string, string]> = {
    weeks: ["Last week", "This week"],
    fortnights: ["The 2 weeks before", "Last 2 weeks"],
    months: ["The 30 days before", "Last 30 days"],
  };
  return [
    { fromDay: today - 2 * len + 1, toDay: today - len, courseId },
    { fromDay: today - len + 1, toDay: today, courseId },
    ...names[preset],
  ];
}

function ComparePage({ nav }: { nav: Nav }) {
  const [preset, setPreset] = useState<Preset>("weeks");
  const { courseId } = useInsightFilter();
  const [a, b, aName, bName] = presetFilters(preset, nav.today, courseId);
  const { data, error } = useLoad(
    () => api.analyticsCompare(a, b),
    [preset, courseId, nav.today],
  );
  return (
    <Page
      title="Compare"
      subtitle={`${aName} and ${bName.toLowerCase()}`}
      nav={nav}
    >
      <div className="insight-segmented">
        <Segmented
          value={preset}
          onChange={setPreset}
          options={[
            { value: "weeks", label: "7 days" },
            { value: "fortnights", label: "14 days" },
            { value: "months", label: "30 days" },
          ]}
        />
      </div>
      {!data ? (
        <Wait error={error} />
      ) : (
        <CompareBody data={data} names={[aName, bName]} nav={nav} />
      )}
    </Page>
  );
}

function CompareBody({
  data,
  names,
  nav,
}: {
  data: Comparison;
  names: [string, string];
  nav: Nav;
}) {
  const open = (c: Comparison["improved"][number]) =>
    c.kind === "structure"
      ? nav.open({ page: "structure", key: c.key })
      : c.kind === "sound"
        ? nav.open({ page: "sounds" })
        : nav.open({ page: "item", itemId: c.key });
  const change = (c: Comparison["improved"][number]) => (
    <ListItem
      key={`${c.kind}-${c.key}`}
      title={c.title}
      subtitle={`${pct(c.accuracyA)} of ${c.attemptsA} → ${pct(c.accuracyB)} of ${c.attemptsB}`}
      trailing={
        <Badge
          tone={c.accuracyB >= c.accuracyA ? "mint" : "crimson"}
        >{`${c.accuracyB >= c.accuracyA ? "+" : "−"}${Math.abs(Math.round((c.accuracyB - c.accuracyA) * 100))}`}</Badge>
      }
      chevron
      onClick={() => open(c)}
    />
  );
  return (
    <div className="insight-board">
      <div className="insight-col">
        <StatGrid>
          <Stat
            value={<CountUp value={data.a.attempts} from={0} />}
            label={`answers · ${names[0].toLowerCase()}`}
          />
          <Stat
            value={<CountUp value={data.b.attempts} from={0} />}
            label={`answers · ${names[1].toLowerCase()}`}
          />
          <Stat
            value={pct(data.a.accuracy)}
            label={`right · ${names[0].toLowerCase()}`}
          />
          <Stat
            value={pct(data.b.accuracy)}
            label={`right · ${names[1].toLowerCase()}`}
          />
        </StatGrid>
        <Section title="Improved">
          {data.improved.length ? (
            <List>{data.improved.map(change)}</List>
          ) : (
            <Text variant="small" tone="dim">
              Nothing clearly better yet (it takes two answers in each period).
            </Text>
          )}
        </Section>
        <Section title="Slipped">
          {data.slipped.length ? (
            <List>{data.slipped.map(change)}</List>
          ) : (
            <Text variant="small" tone="dim">
              Nothing slipped.
            </Text>
          )}
        </Section>
      </div>
      <div className="insight-col">
        <Section title="Mistakes, before and now">
          <List>
            {data.mistakes.map((m) => (
              <ListItem
                key={m.class}
                title={m.label}
                subtitle={`${m.a} → ${m.b}`}
                chevron
                onClick={() => nav.open({ page: "mistakes", cls: m.class })}
              />
            ))}
          </List>
        </Section>
      </div>
    </div>
  );
}

// ------------------------------------------------------------ activity

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

function ActivityPage({ nav }: { nav: Nav }) {
  const filter = useFilter(nav);
  const { data, error } = useLoad(
    () => api.analyticsActivity(filter),
    [filter],
  );
  return (
    <Page title="Activity and habits" nav={nav}>
      <Filters nav={nav} />
      {!data ? <Wait error={error} /> : <ActivityBody data={data} nav={nav} />}
    </Page>
  );
}

function ActivityBody({ data, nav }: { data: Activity; nav: Nav }) {
  const hourMax = Math.max(1, ...data.byHour);
  const dayMax = Math.max(1, ...data.byWeekday);
  const hours: Bar[] = data.byHour.map((n, h) => ({
    key: h,
    label: `${h}:00: ${n} answers`,
    value: n,
    share: n / hourMax,
    empty: n === 0,
  }));
  const days: Bar[] = data.byWeekday.map((n, d) => ({
    key: d,
    label: `${WEEKDAYS[d]}: ${n} answers`,
    value: n,
    share: n / dayMax,
    empty: n === 0,
  }));
  const busiest = data.byHour.indexOf(Math.max(...data.byHour));
  return (
    <div className="insight-board">
      <div className="insight-col">
        <StatGrid>
          <Stat
            value={<CountUp value={data.daysActive} memory="activity:days" />}
            label="days practised"
          />
          <Stat
            value={<CountUp value={data.minutes} memory="activity:minutes" />}
            label="minutes"
          />
          <Stat
            value={<CountUp value={data.sessions} memory="activity:started" />}
            label="drills started"
          />
          <Stat
            value={<CountUp value={data.leftEarly} memory="activity:left" />}
            label="left early"
          />
        </StatGrid>
        <Section
          title="When you practise"
          info={<p>Answers by hour of the day, in your time.</p>}
        >
          <Card>
            <BarChart
              bars={hours}
              label="Answers by hour of the day"
              start="Midnight"
              end="11 pm"
            />
            {data.attempts ? (
              <Text variant="small" tone="dim">
                Busiest around {busiest}:00.
              </Text>
            ) : null}
          </Card>
        </Section>
        <Section title="By weekday">
          <Card>
            <BarChart
              bars={days}
              label="Answers by weekday"
              start="Mon"
              end="Sun"
              small
            />
          </Card>
        </Section>
      </div>
      <div className="insight-col">
        <Section title="How you answer">
          <List>
            <ListItem
              title="Time to answer"
              trailing={
                <Text tone="dim">
                  {data.medianMs === null
                    ? "—"
                    : `${(data.medianMs / 1000).toFixed(1)} s`}
                </Text>
              }
            />
            <ListItem
              title="Polish played again"
              trailing={<Text tone="dim">{data.audioPlays}</Text>}
            />
            <ListItem
              title="Translations shown"
              trailing={<Text tone="dim">{data.reveals}</Text>}
            />
            <ListItem
              title="Drills finished"
              trailing={<Text tone="dim">{data.finished}</Text>}
            />
            <ListItem
              title="Quick games"
              trailing={<Text tone="dim">{data.games}</Text>}
            />
            <ListItem
              title="Check-ins"
              trailing={<Text tone="dim">{data.checkins}</Text>}
            />
          </List>
        </Section>
        {data.exits.length ? (
          <Section title="Left early">
            <List>
              {data.exits.slice(0, 8).map((e, i) => (
                <ListItem
                  key={`${e.at}-${i}`}
                  title={e.title || "A lesson"}
                  subtitle={`${whenLabel(e.at, nav.today)}${e.atCard ? ` · at card ${e.atCard} of ${e.ofCards ?? "?"}` : ""}`}
                />
              ))}
            </List>
          </Section>
        ) : null}
        {data.lessons.length ? (
          <Section title="Practised most">
            <List>
              {data.lessons.map((l) => (
                <ListItem
                  key={l.lessonId}
                  title={l.title}
                  subtitle={`${l.attempts} answers · ${pct(l.accuracy)} right`}
                />
              ))}
            </List>
          </Section>
        ) : null}
      </div>
    </div>
  );
}

// ------------------------------------------------------------ one item

function ItemPage({ nav, itemId }: { nav: Nav; itemId: string }) {
  const { data, error } = useLoad(() => api.analyticsItem(itemId), [itemId]);
  const [talking, setTalking] = useState(false);
  if (!data) {
    return (
      <Page title="History" nav={nav}>
        <Wait error={error} />
      </Page>
    );
  }
  const talkable = data.itemKind === "word" || data.itemKind === "sentence";
  return (
    <Page title={data.pl} subtitle={data.en} nav={nav}>
      <div className="insight-board">
        <div className="insight-col">
          <Card>
            <Stack gap={3}>
              <Inline justify="between" gap={2}>
                <Text variant="title" tone="polish" selectable>
                  {data.pl}
                </Text>
                <StrengthBadge strength={data.strength} />
              </Inline>
              <Text tone="dim" selectable>
                {data.en}
              </Text>
              <StatGrid>
                <Stat
                  value={<CountUp value={data.attempts} from={0} />}
                  label="answers"
                />
                <Stat
                  value={<CountUp value={data.wrong} from={0} />}
                  label="missed"
                />
              </StatGrid>
              {talkable ? (
                <Button
                  icon={<span aria-hidden="true">✨</span>}
                  onClick={() => setTalking(true)}
                >
                  Ask the tutor about it
                </Button>
              ) : null}
            </Stack>
          </Card>
          <Section title="Every answer">
            <Attempts
              attempts={data.history}
              nav={nav}
              linked={false}
              empty="No answers yet."
            />
          </Section>
        </div>
        <div className="insight-col">
          <Section title="Typical mistakes">
            <Card>
              <MistakeMeasures
                counts={data.mistakes}
                nav={nav}
                memory={`item:${itemId}`}
              />
            </Card>
          </Section>
          {data.grammar.length ? (
            <Section title="Grammar">
              <Inline gap={2} wrap>
                {data.grammar.map((t) => (
                  <Badge
                    key={t.key}
                    tone="brand"
                    onClick={() => nav.open({ page: "structure", key: t.key })}
                  >
                    {t.name}
                  </Badge>
                ))}
              </Inline>
            </Section>
          ) : null}
          <Events history={data} today={nav.today} />
        </div>
      </div>
      {talking ? (
        <WordChat
          itemKind={data.itemKind}
          itemId={data.itemId}
          title={data.pl}
          onClose={() => setTalking(false)}
        />
      ) : null}
    </Page>
  );
}

/** For Progress: everything met, in three numbers. */
export function useMetOverview() {
  return useLoad(() => api.analyticsOverview(ALL_TIME), []);
}
