import { useEffect, useMemo, useState } from "react";

import {
  Banner,
  List,
  ListEmpty,
  ListItem,
  Inline,
  Section,
  Segmented,
  Sheet,
  Spinner,
  Stack,
  Switch,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import type { AiModel, BenchRun, FeatureUsage } from "../lib/types";
import { StarIcon } from "./icons";

// The catalogue is the same for every picker: fetched once per app run.
let catalogue: Promise<AiModel[]> | null = null;
function loadCatalogue(): Promise<AiModel[]> {
  catalogue ??= api.aiListModels().catch((cause) => {
    catalogue = null;
    throw cause;
  });
  return catalogue;
}

/** Dollars per token → "$0.30" per million tokens; "free"; "varies" for routers. */
function perMillion(price: string): string {
  const value = Number.parseFloat(price);
  if (!Number.isFinite(value) || value < 0) return "varies";
  if (value === 0) return "free";
  const million = value * 1_000_000;
  return `$${million < 0.1 ? million.toFixed(3) : million < 10 ? million.toFixed(2) : million.toFixed(0)}`;
}

/** "$0.30 in · $2.50 out" per million tokens. */
function priceLine(model: AiModel): string {
  const input = perMillion(model.promptPrice);
  const output = perMillion(model.completionPrice);
  if (input === "free" && output === "free") return "Free";
  return `${input} in · ${output} out`;
}

function contextLine(tokens: number): string | null {
  if (!tokens) return null;
  if (tokens >= 1_000_000) return `${+(tokens / 1_000_000).toFixed(1)}M context`;
  return `${Math.round(tokens / 1000)}k context`;
}

/** What a model costs to use, for sorting: a turn reads more than it writes. */
function weight(model: AiModel): number {
  const input = Number.parseFloat(model.promptPrice);
  const output = Number.parseFloat(model.completionPrice);
  if (!Number.isFinite(input) || input < 0) return Number.POSITIVE_INFINITY;
  return 3 * input + (Number.isFinite(output) && output > 0 ? output : 0);
}

type Need = "tools" | "images" | "any";

/** Which setting a picker is for: what it is estimated from and recommended for. */
export type Job = "tutor" | "songwriter" | "partner" | "word chat" | "pictures";

/** The cost-log features each job's model pays for. */
const JOB_FEATURES: Record<Job, string[]> = {
  // The main model also runs check-ins.
  tutor: ["tutor", "examiner"],
  songwriter: ["songwriter"],
  partner: ["writing partner"],
  "word chat": ["word chat"],
  pictures: [],
};

// Recent token use, fetched once per app run like the catalogue.
let usageLoad: Promise<FeatureUsage[]> | null = null;
function loadUsage(): Promise<FeatureUsage[]> {
  usageLoad ??= api.aiUsage().catch(() => {
    usageLoad = null;
    return [];
  });
  return usageLoad;
}

// Benchmark scores, newest run per model.
let benchLoad: Promise<BenchRun[]> | null = null;
function loadBench(): Promise<BenchRun[]> {
  benchLoad ??= api.benchRuns().catch(() => {
    benchLoad = null;
    return [];
  });
  return benchLoad;
}

/**
 * What a month of this job would cost with `model`, from the tokens it used
 * lately (cached input at the model's cache price). Null without data.
 */
function monthly(model: AiModel, job: Job, usage: FeatureUsage[]): number | null {
  const rows = usage.filter((u) => JOB_FEATURES[job].includes(u.feature));
  if (rows.length === 0) return null;
  const price = (value: string) => {
    const n = Number.parseFloat(value);
    return Number.isFinite(n) && n >= 0 ? n : null;
  };
  const input = price(model.promptPrice);
  const output = price(model.completionPrice);
  if (input === null || output === null) return null;
  const cacheRead = price(model.cacheReadPrice) ?? input;
  let total = 0;
  for (const row of rows) {
    const perDay =
      ((row.tokensIn - row.tokensCached) * input + row.tokensCached * cacheRead + row.tokensOut * output) /
      Math.max(1, row.days);
    total += perDay * 30;
  }
  return total;
}

function monthlyLine(amount: number | null): string | null {
  if (amount === null) return null;
  if (amount < 0.01) return "≈ under 1¢/month for you";
  return `≈ $${amount < 10 ? amount.toFixed(2) : amount.toFixed(0)}/month for you`;
}

/** Model families by what they are good for, preferred first. */
const FAMILIES = {
  quality: [/^anthropic\/claude-sonnet/, /^google\/gemini-[\d.]+-pro$/, /^openai\/gpt-5(\.\d+)?$/],
  value: [/^google\/gemini-[\d.]+-flash$/, /^anthropic\/claude-haiku/, /^openai\/gpt-5(\.\d+)?-mini$/],
  light: [/^google\/gemini-[\d.]+-flash-lite$/, /^openai\/gpt-5(\.\d+)?-nano$/, /^anthropic\/claude-haiku/],
} as const;

const PICKS: Record<Job, { family: keyof typeof FAMILIES; why: string }[]> = {
  tutor: [
    { family: "value", why: "Best value — good Polish and reliable tools at a fraction of the price" },
    { family: "quality", why: "Top quality — for the courses and lessons you keep" },
  ],
  songwriter: [
    { family: "value", why: "Best value — plenty for lyrics" },
    { family: "quality", why: "Top quality — the most natural lyrics" },
  ],
  partner: [
    { family: "light", why: "Light and fast — plenty for short replies in character" },
    { family: "value", why: "A step up — more natural conversation" },
  ],
  "word chat": [
    { family: "light", why: "Light and fast — plenty for a short chat about a word" },
    { family: "value", why: "A step up — richer explanations" },
  ],
  pictures: [],
};

/**
 * The recommended models for a job from what the catalogue has now: the
 * newest model of the first family that has one, per pick.
 */
function recommended(models: AiModel[], job: Job): { model: AiModel; why: string }[] {
  const usable = models.filter(
    (m) => m.tools && !m.id.includes(":") && !/preview|exp|image|audio|tts/.test(m.id),
  );
  const picks: { model: AiModel; why: string }[] = [];
  for (const { family, why } of PICKS[job]) {
    for (const pattern of FAMILIES[family]) {
      const newest = usable.filter((m) => pattern.test(m.id)).sort((a, b) => b.created - a.created)[0];
      if (newest) {
        if (!picks.some((p) => p.model.id === newest.id)) picks.push({ model: newest, why });
        break;
      }
    }
  }
  return picks;
}

/**
 * A model setting as one row: the chosen model and its price. Tapping it
 * opens a picker with search, favourites first, prices per million tokens
 * and a cheapest-first sort.
 */
export function ModelSetting({
  label,
  value,
  fallback,
  need = "any",
  job,
  favorites,
  onFavorites,
  onChange,
}: {
  label: string;
  /** Which setting: what the estimate and recommendations are for. */
  job: Job;
  value: string;
  /** What empty means ("Same as the tutor"); absent when a model is required. */
  fallback?: string;
  /** What the job needs: tool use (acting in the app), image output, or nothing. */
  need?: Need;
  favorites: string[];
  onFavorites: (favorites: string[]) => void;
  onChange: (model: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [models, setModels] = useState<AiModel[] | null>(null);
  const [usage, setUsage] = useState<FeatureUsage[]>([]);
  useEffect(() => {
    // Known models give the row a name and price; without them it shows the id.
    loadCatalogue()
      .then(setModels)
      .catch(() => {});
    void loadUsage().then(setUsage);
  }, []);
  const current = models?.find((model) => model.id === value);
  const estimate = current ? monthlyLine(monthly(current, job, usage)) : null;

  return (
    <>
      <ListItem
        title={label}
        subtitle={
          value
            ? `${current?.name ?? value}${current ? ` · ${estimate ?? priceLine(current)}` : ""}`
            : (fallback ?? "None chosen yet")
        }
        onClick={() => setOpen(true)}
      />
      {open ? (
        <ModelPicker
          title={label}
          value={value}
          fallback={fallback}
          need={need}
          job={job}
          usage={usage}
          favorites={favorites}
          onFavorites={onFavorites}
          onPick={(model) => {
            onChange(model);
            setOpen(false);
          }}
          onClose={() => setOpen(false)}
        />
      ) : null}
    </>
  );
}

function ModelPicker({
  title,
  value,
  fallback,
  need,
  job,
  usage,
  favorites,
  onFavorites,
  onPick,
  onClose,
}: {
  title: string;
  value: string;
  fallback?: string;
  need: Need;
  job: Job;
  usage: FeatureUsage[];
  favorites: string[];
  onFavorites: (favorites: string[]) => void;
  onPick: (model: string) => void;
  onClose: () => void;
}) {
  const [models, setModels] = useState<AiModel[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [bench, setBench] = useState<BenchRun[]>([]);
  useEffect(() => {
    benchLoad = null;
    void loadBench().then(setBench);
  }, []);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<"name" | "price">("name");
  // Acting in the app needs tool use: only those, unless asked for all.
  const [toolsOnly, setToolsOnly] = useState(need === "tools");

  useEffect(() => {
    loadCatalogue()
      .then(setModels)
      .catch((cause) => setError(errorMessage(cause)));
  }, []);

  const shown = useMemo(() => {
    if (!models) return [];
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    const list = models.filter((model) => {
      if (need === "images" ? !model.images : model.voices.length > 0) return false;
      if (toolsOnly && !model.tools) return false;
      const haystack = `${model.name} ${model.id}`.toLowerCase();
      return words.every((word) => haystack.includes(word));
    });
    return sort === "price" ? [...list].sort((a, b) => weight(a) - weight(b)) : list;
  }, [models, query, sort, need, toolsOnly]);

  const picks = models && !query ? recommended(models, job) : [];
  const starred = shown.filter((model) => favorites.includes(model.id));
  const rest = shown.filter((model) => !favorites.includes(model.id));

  function toggle(id: string) {
    onFavorites(favorites.includes(id) ? favorites.filter((f) => f !== id) : [...favorites, id]);
  }

  function row(model: AiModel, why?: string) {
    const on = favorites.includes(model.id);
    const estimate = monthlyLine(monthly(model, job, usage));
    // Runs come newest first: the first for a model is its latest.
    const scored = bench.find((run) => run.model === model.id);
    return (
      <ListItem
        key={`${why ? "pick-" : ""}${model.id}`}
        selected={model.id === value}
        title={`${model.tools && need !== "images" ? "🛠 " : ""}${model.name}`}
        subtitle={
          <>
            {why ? (
              <>
                ★ {why}
                <br />
              </>
            ) : null}
            {[scored ? `Benchmark ${scored.score}` : null, estimate, priceLine(model), estimate ? null : contextLine(model.contextLength)]
              .filter(Boolean)
              .join(" · ")}
          </>
        }
        onClick={() => onPick(model.id)}
        trailing={
          <button
            className="model-star"
            data-on={on || undefined}
            aria-label={on ? `Unstar ${model.name}` : `Star ${model.name}`}
            aria-pressed={on}
            onClick={(event) => {
              event.stopPropagation();
              toggle(model.id);
            }}
          >
            <StarIcon on={on} />
          </button>
        }
      />
    );
  }

  return (
    <Sheet title={title} onClose={onClose}>
      <Stack gap={3}>
        <input
          type="search"
          value={query}
          placeholder="Search models"
          aria-label="Search models"
          onChange={(event) => setQuery(event.target.value)}
        />
        <Segmented
          value={sort}
          options={[
            { value: "name", label: "A–Z" },
            { value: "price", label: "Cheapest" },
          ]}
          onChange={setSort}
        />
        {need === "tools" ? (
          <List>
            <ToolsOnlyRow on={toolsOnly} onChange={setToolsOnly} />
          </List>
        ) : null}
        <Text variant="small" tone="faint">
          {usage.some((u) => JOB_FEATURES[job].includes(u.feature))
            ? "“For you” is what a month would cost with that model, from how much you have used this lately. Prices are per million tokens."
            : "Prices are per million tokens. Once you have used this a little, each model shows what a month would cost you."}
        </Text>
        {error ? <Banner tone="error">{error}</Banner> : null}
        {models === null && !error ? (
          <Inline gap={2}>
            <Spinner />
            <Text variant="small" tone="faint">
              Loading models…
            </Text>
          </Inline>
        ) : null}
        {fallback && !query ? (
          <List>
            <ListItem title={fallback} selected={value === ""} onClick={() => onPick("")} />
          </List>
        ) : null}
        {picks.length ? (
          <Section title="Recommended">
            <List>{picks.map((pick) => row(pick.model, pick.why))}</List>
          </Section>
        ) : null}
        {starred.length ? (
          <Section title="Favourites">
            <List>{starred.map((model) => row(model))}</List>
          </Section>
        ) : null}
        {models ? (
          <Section title={starred.length ? "All models" : `${shown.length} models`}>
            <List>{rest.length ? rest.map((model) => row(model)) : <ListEmpty>No model matches.</ListEmpty>}</List>
          </Section>
        ) : null}
      </Stack>
    </Sheet>
  );
}

function ToolsOnlyRow({ on, onChange }: { on: boolean; onChange: (on: boolean) => void }) {
  return (
    <ListItem
      title="Only models that can use tools 🛠"
      subtitle="Needed to build courses, drills and songs"
      trailing={<Switch checked={on} onChange={onChange} label="Only models that can use tools" />}
    />
  );
}
