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
import type { AiModel } from "../lib/types";
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
  favorites,
  onFavorites,
  onChange,
}: {
  label: string;
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
  useEffect(() => {
    // Known models give the row a name and price; without them it shows the id.
    loadCatalogue()
      .then(setModels)
      .catch(() => {});
  }, []);
  const current = models?.find((model) => model.id === value);

  return (
    <>
      <ListItem
        title={label}
        subtitle={
          value
            ? `${current?.name ?? value}${current ? ` · ${priceLine(current)}` : ""}`
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
  favorites,
  onFavorites,
  onPick,
  onClose,
}: {
  title: string;
  value: string;
  fallback?: string;
  need: Need;
  favorites: string[];
  onFavorites: (favorites: string[]) => void;
  onPick: (model: string) => void;
  onClose: () => void;
}) {
  const [models, setModels] = useState<AiModel[] | null>(null);
  const [error, setError] = useState<string | null>(null);
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

  const starred = shown.filter((model) => favorites.includes(model.id));
  const rest = shown.filter((model) => !favorites.includes(model.id));

  function toggle(id: string) {
    onFavorites(favorites.includes(id) ? favorites.filter((f) => f !== id) : [...favorites, id]);
  }

  function row(model: AiModel) {
    const on = favorites.includes(model.id);
    return (
      <ListItem
        key={model.id}
        selected={model.id === value}
        title={`${model.tools && need !== "images" ? "🛠 " : ""}${model.name}`}
        subtitle={[priceLine(model), contextLine(model.contextLength)].filter(Boolean).join(" · ")}
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
          Prices are per million tokens. A reply here is usually a few thousand.
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
        {starred.length ? (
          <Section title="Favourites">
            <List>{starred.map(row)}</List>
          </Section>
        ) : null}
        {models ? (
          <Section title={starred.length ? "All models" : `${shown.length} models`}>
            <List>{rest.length ? rest.map(row) : <ListEmpty>No model matches.</ListEmpty>}</List>
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
