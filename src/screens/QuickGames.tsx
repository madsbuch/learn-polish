import { useEffect, useMemo, useRef, useState } from "react";

import { Memory } from "../components/SetExercises";
import {
  Avatar,
  Badge,
  Banner,
  Button,
  Card,
  Confetti,
  CountUp,
  EmptyState,
  List,
  ListItem,
  Loading,
  Screen,
  Section,
  Stack,
  Text,
  markBack,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { hapticRight, hapticTick, hapticWrong } from "../lib/haptics";
import { playComplete, playMatch, playWrong } from "../lib/sounds";
import type { GameFinished, Pair, QuickGame, QuickGameKind, QuickGames as Overview } from "../lib/types";

/*
 * Quick games, from Home: a minute's play over words already met, outside
 * the daily five (games.rs). Match pairs is timed; Flip and match counts
 * moves. They pay a little XP, capped a day, and leave the schedule alone.
 */

const GAMES: { kind: QuickGameKind; title: string; icon: string; blurb: string }[] = [
  { kind: "match", title: "Match pairs", icon: "🔗", blurb: "Eight pairs against the clock" },
  { kind: "flip", title: "Flip and match", icon: "🃏", blurb: "Memory cards: a word and its picture" },
];

/** "0:42" — or, while it runs, "0:42.3". */
export function clock(ms: number, tenths = false) {
  const total = Math.max(0, ms);
  const minutes = Math.floor(total / 60000);
  const seconds = Math.floor((total % 60000) / 1000);
  const tenth = Math.floor((total % 1000) / 100);
  return `${minutes}:${String(seconds).padStart(2, "0")}${tenths ? `.${tenth}` : ""}`;
}

/** What the menu says of a game's best. */
function bestLine(overview: Overview, kind: QuickGameKind) {
  if (kind === "match") return overview.matchBestMs !== null ? `Best ${clock(overview.matchBestMs)}` : null;
  return overview.flipBestMoves !== null ? `Best ${overview.flipBestMoves} moves` : null;
}

export function QuickGames({
  level,
  onBack,
  onPlayed,
}: {
  /** The learner's level now: a game that takes them past it celebrates. */
  level: number;
  onBack: () => void;
  onPlayed: () => void;
}) {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [playing, setPlaying] = useState<QuickGameKind | null>(null);

  useEffect(() => {
    if (playing !== null) return;
    api
      .quickGames()
      .then(setOverview)
      .catch((cause) => setError(errorMessage(cause)));
  }, [playing]);

  if (playing !== null) {
    return (
      <GameScreen
        kind={playing}
        level={level}
        onBack={() => {
          markBack();
          setPlaying(null);
        }}
        onPlayed={onPlayed}
      />
    );
  }

  const enough = overview ? overview.wordsMet >= overview.minWords : false;
  return (
    <Screen
      title="Quick games"
      onBack={onBack}
      nav={false}
      info={
        <>
          <p>
            A minute&apos;s play with words you have already met in your lessons, whenever you like — outside
            today&apos;s lessons.
          </p>
          <p>
            Each game finished earns a little XP, up to a few games&apos; worth a day, so the lessons stay the main
            thing. The games don&apos;t change when a word comes back: matching from cards you can see is a good
            warm-up, not proof you remember it — the lessons decide that.
          </p>
        </>
      }
    >
      {error ? <Banner tone="error">{error}</Banner> : null}
      {!overview ? (
        error ? null : (
          <Loading />
        )
      ) : !enough ? (
        <EmptyState icon="🃏" title="Meet a few words first">
          The games use words from your lessons. Do a lesson or two, and they are ready.
        </EmptyState>
      ) : (
        <>
          <Section title="Play">
            <List stagger>
              {GAMES.map((game) => (
                <ListItem
                  key={game.kind}
                  leading={<Avatar>{game.icon}</Avatar>}
                  title={game.title}
                  subtitle={[game.blurb, bestLine(overview, game.kind)].filter(Boolean).join(" · ")}
                  onClick={() => {
                    hapticTick();
                    setPlaying(game.kind);
                  }}
                />
              ))}
            </List>
          </Section>
          <Text variant="small" tone="dim" align="center" as="p">
            {overview.xpToday >= overview.xpPerDay
              ? `Today's ${overview.xpPerDay} XP from games earned — play on for fun.`
              : `+${overview.xpPerGame} XP a game, up to ${overview.xpPerDay} a day · ${overview.xpToday} earned today`}
          </Text>
        </>
      )}
    </Screen>
  );
}

/** One game: dealt, played against the clock, then its result. */
function GameScreen({
  kind,
  level,
  onBack,
  onPlayed,
}: {
  kind: QuickGameKind;
  level: number;
  onBack: () => void;
  onPlayed: () => void;
}) {
  // The level as the game began: the result says if it went up.
  const [startLevel] = useState(level);
  const title = GAMES.find((g) => g.kind === kind)?.title ?? "Quick game";
  const [deal, setDeal] = useState<QuickGame | null>(null);
  const [round, setRound] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [endedAt, setEndedAt] = useState<number | null>(null);
  const [moves, setMoves] = useState(0);
  const [result, setResult] = useState<GameFinished | null>(null);

  useEffect(() => {
    setDeal(null);
    setResult(null);
    setStartedAt(null);
    setEndedAt(null);
    setMoves(0);
    api
      .quickGameDeal(kind)
      .then(setDeal)
      .catch((cause) => setError(errorMessage(cause)));
  }, [kind, round]);

  function start() {
    setStartedAt((at) => at ?? performance.now());
  }

  async function finish(finalMoves: number) {
    const now = performance.now();
    setEndedAt(now);
    const ms = Math.round(now - (startedAt ?? now));
    try {
      const finished = await api.quickGameFinish(kind, deal?.pairs.length ?? 0, ms, finalMoves);
      setResult(finished);
      playComplete();
      onPlayed();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }

  const elapsed = startedAt === null ? 0 : (endedAt ?? 0) - startedAt;
  return (
    <Screen title={title} onBack={onBack} nav={false}>
      {error ? <Banner tone="error">{error}</Banner> : null}
      <div className="game-bar">
        <Badge tone="sky" label="Time">
          ⏱ {endedAt !== null || startedAt === null ? clock(elapsed, true) : <Clock since={startedAt} />}
        </Badge>
        <Badge tone="neutral">{kind === "match" ? `${moves} slip${moves === 1 ? "" : "s"}` : `${moves} move${moves === 1 ? "" : "s"}`}</Badge>
      </div>
      {result ? (
        <GameResult
          kind={kind}
          ms={elapsed}
          moves={moves}
          result={result}
          levelUp={result.level > startLevel}
          onAgain={() => setRound((n) => n + 1)}
          onBack={onBack}
        />
      ) : !deal ? (
        error ? null : (
          <Loading />
        )
      ) : kind === "match" ? (
        <MatchPairs
          key={round}
          pairs={deal.pairs}
          onStart={start}
          onSlip={() => setMoves((n) => n + 1)}
          onDone={() => void finish(moves)}
        />
      ) : (
        <>
          <Text variant="eyebrow">Turn two cards: a word and its picture or meaning</Text>
          <Memory
            pairs={deal.pairs}
            deal={`quick-${round}`}
            onTurn={start}
            onComplete={(turns) => {
              setMoves(turns);
              void finish(turns);
            }}
          />
        </>
      )}
    </Screen>
  );
}

/** The running clock: its own frame loop writes the text, so the board isn't redrawn every frame. */
function Clock({ since }: { since: number }) {
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    let frame = 0;
    const tick = () => {
      if (ref.current) ref.current.textContent = clock(performance.now() - since, true);
      frame = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(frame);
  }, [since]);
  return <span ref={ref} className="game-clock" />;
}

function shuffled<T>(items: T[]): T[] {
  const copy = [...items];
  for (let i = copy.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [copy[i], copy[j]] = [copy[j], copy[i]];
  }
  return copy;
}

/**
 * Match pairs: the Polish words on the left, their meanings on the right,
 * each column shuffled. Tap one of each; a pair stays matched, anything else
 * shakes and counts as a slip.
 */
function MatchPairs({
  pairs,
  onStart,
  onSlip,
  onDone,
}: {
  pairs: Pair[];
  onStart: () => void;
  onSlip: () => void;
  onDone: () => void;
}) {
  const left = useMemo(() => shuffled(pairs.map((_, i) => i)), [pairs]);
  const right = useMemo(() => shuffled(pairs.map((_, i) => i)), [pairs]);
  const [picked, setPicked] = useState<{ side: "pl" | "en"; pair: number } | null>(null);
  const [found, setFound] = useState<Set<number>>(new Set());
  const [slip, setSlip] = useState<{ pl: number; en: number } | null>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  function tap(side: "pl" | "en", pair: number) {
    if (found.has(pair) || slip) return;
    onStart();
    if (!picked || picked.side === side) {
      hapticTick();
      setPicked(picked?.side === side && picked.pair === pair ? null : { side, pair });
      return;
    }
    setPicked(null);
    if (picked.pair === pair) {
      const next = new Set(found).add(pair);
      setFound(next);
      playMatch();
      hapticRight();
      if (next.size === pairs.length) onDone();
      return;
    }
    playWrong();
    hapticWrong();
    onSlip();
    setSlip(side === "pl" ? { pl: pair, en: picked.pair } : { pl: picked.pair, en: pair });
    timer.current = window.setTimeout(() => setSlip(null), 450);
  }

  const state = (side: "pl" | "en", pair: number) =>
    found.has(pair)
      ? "found"
      : slip && slip[side] === pair
        ? "slip"
        : picked?.side === side && picked.pair === pair
          ? "picked"
          : "idle";

  return (
    <>
      <Text variant="eyebrow">Tap a Polish word, then its meaning</Text>
      <div className="match-grid">
        <div className="match-column">
          {left.map((pair) => (
            <button
              key={pair}
              className="match-tile"
              data-lang="pl"
              data-state={state("pl", pair)}
              disabled={found.has(pair)}
              onClick={() => tap("pl", pair)}
            >
              {pairs[pair].pl}
            </button>
          ))}
        </div>
        <div className="match-column">
          {right.map((pair) => (
            <button
              key={pair}
              className="match-tile"
              data-lang="en"
              data-state={state("en", pair)}
              disabled={found.has(pair)}
              onClick={() => tap("en", pair)}
            >
              {pairs[pair].en}
            </button>
          ))}
        </div>
      </div>
      <Text variant="small" tone="faint" align="center" as="div">
        {found.size} of {pairs.length} pairs
      </Text>
    </>
  );
}

/** How a game went: the time or moves, the best, the XP. A record gets confetti. */
function GameResult({
  kind,
  ms,
  moves,
  result,
  levelUp,
  onAgain,
  onBack,
}: {
  kind: QuickGameKind;
  ms: number;
  moves: number;
  result: GameFinished;
  levelUp: boolean;
  onAgain: () => void;
  onBack: () => void;
}) {
  const headline = kind === "match" ? clock(ms, true) : `${moves} moves`;
  const before =
    kind === "match"
      ? result.previousMs !== null
        ? `Best before: ${clock(result.previousMs, true)}`
        : "Your first game: the time to beat."
      : result.previousMoves !== null
        ? `Best before: ${result.previousMoves} moves`
        : "Your first game: the moves to beat.";
  const best =
    kind === "match"
      ? result.bestMs !== null && !result.record
        ? `Best ${clock(result.bestMs, true)}`
        : null
      : result.bestMoves !== null && !result.record
        ? `Best ${result.bestMoves} moves`
        : null;
  return (
    <>
      {result.record || levelUp ? <Confetti fixed pieces={36} /> : null}
      <Card className="game-result pop">
        <Stack gap={2} align="center">
          <Text variant="display">{result.record ? "🏆" : "👏"}</Text>
          <Text variant="headline" weight="bold" className={result.record ? "celebrate-rise" : undefined}>
            {headline}
          </Text>
          {result.record ? <Badge tone="gold">A new best!</Badge> : null}
          {levelUp ? <Badge tone="brand">Level {result.level}!</Badge> : null}
          <Text variant="small" tone="dim" align="center" as="div">
            {[best, kind === "flip" ? `in ${clock(ms)}` : null, before].filter(Boolean).join(" · ")}
          </Text>
          <Badge tone={result.xpGained > 0 ? "gold" : "neutral"}>
            {result.xpGained > 0 ? (
              <>
                +<CountUp value={result.xpGained} from={0} /> XP
              </>
            ) : (
              `Today's ${result.xpPerDay} XP from games earned`
            )}
          </Badge>
        </Stack>
      </Card>
      <Stack gap={2}>
        <Button variant="primary" size="lg" wide onClick={onAgain}>
          Play again
        </Button>
        <Button variant="plain" wide onClick={onBack}>
          Back to games
        </Button>
      </Stack>
    </>
  );
}
