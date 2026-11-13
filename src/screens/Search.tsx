import { useEffect, useMemo, useState } from "react";

import { SpeakButton } from "../components/ui";
import {
  Banner,
  Choice,
  ChoiceRow,
  List,
  ListEmpty,
  ListItem,
  Screen,
  Section,
  Segmented,
  Spinner,
  Stack,
} from "../design";
import { api, errorMessage } from "../lib/api";
import type { CataloguePage, TopicProgress } from "../lib/types";

type Kind = "words" | "sentences";

/** Enough to scroll through, few enough to stay responsive on a phone. */
const PAGE = 200;

/** Every word and sentence in the app, searchable in Polish or English. */
export function Search({
  topics,
  onBack,
}: {
  topics: TopicProgress[];
  onBack: () => void;
}) {
  const [section, setSection] = useState<Kind>("words");
  const [query, setQuery] = useState("");
  const [topicId, setTopicId] = useState<number | null>(null);
  const [page, setPage] = useState<CataloguePage | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Debounced so typing does not fire a query per keystroke.
  useEffect(() => {
    let cancelled = false;
    const timer = setTimeout(() => {
      api
        .browseCatalogue(topicId, query, PAGE)
        .then((result) => {
          if (!cancelled) setPage(result);
        })
        .catch((cause) => {
          if (!cancelled) setError(errorMessage(cause));
        });
    }, 180);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query, topicId]);

  const shown = useMemo(() => {
    if (!page) return { rows: [], total: 0 };
    return section === "sentences"
      ? { rows: page.sentences, total: page.totalSentences }
      : { rows: page.words, total: page.totalWords };
  }, [page, section]);

  return (
    <Screen title="Search" subtitle="Every word and sentence in the app" onBack={onBack} nav={false}>
      {error ? <Banner tone="error">{error}</Banner> : null}

      <Stack gap={3}>
        <input
          type="search"
          value={query}
          placeholder={`Search ${section} in Polish or English`}
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          onChange={(event) => setQuery(event.target.value)}
        />
        <Segmented
          value={section}
          onChange={setSection}
          options={[
            { value: "words", label: "Words" },
            { value: "sentences", label: "Sentences" },
          ]}
        />
        <ChoiceRow>
          <Choice selected={topicId === null} onClick={() => setTopicId(null)}>
            All topics
          </Choice>
          {topics.map((entry) => (
            <Choice
              key={entry.topic.id}
              selected={topicId === entry.topic.id}
              onClick={() => setTopicId(entry.topic.id)}
            >
              {entry.topic.title}
            </Choice>
          ))}
        </ChoiceRow>
      </Stack>

      {!page ? (
        <Spinner />
      ) : (
        <Section
          title={`${shown.total} ${section}${topicId !== null || query.trim() ? " matching" : ""}${
            shown.total > shown.rows.length ? ` · first ${shown.rows.length}` : ""
          }`}
        >
          <List>
            {shown.rows.map((row) => (
              <ListItem
                key={row.id}
                selectable
                title={row.pl}
                subtitle={"pos" in row ? `${row.meaning} · ${row.pos}` : row.meaning}
                trailing={<SpeakButton text={row.pl} />}
              />
            ))}
            {shown.rows.length === 0 ? <ListEmpty>Nothing matches “{query.trim()}”.</ListEmpty> : null}
          </List>
        </Section>
      )}
    </Screen>
  );
}
