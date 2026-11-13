import { BackIcon } from "../components/icons";
import { useEffect, useState } from "react";

import { Info } from "../components/Info";
import { Banner, Spinner } from "../components/ui";
import { api, errorMessage } from "../lib/api";
import { sectionsToText, textToSections } from "../lib/lyrics";
import type { Sentence, Settings, SongDraft, TopicProgress } from "../lib/types";

// ------------------------------------------------------------------ composer

const STYLE_PRESETS = [
  "catchy acoustic pop, mid tempo, warm female vocals, clear diction",
  "upbeat indie folk, hand claps, male vocals, clear diction",
  "80s synth pop, bright and danceable, clear vocals, clear diction",
  "gentle lullaby, slow, soft piano, clear vocals, clear diction",
  "reggae, relaxed groove, clear vocals, clear diction",
  "pop rock, driving guitars, anthemic chorus, clear vocals, clear diction",
];
const PRESET_NAMES = ["Acoustic pop", "Indie folk", "Synth pop", "Lullaby", "Reggae", "Pop rock"];

export function SongComposer({
  settings,
  topics,
  onCancel,
  onCreated,
}: {
  settings: Settings;
  topics: TopicProgress[];
  onCancel: () => void;
  onCreated: () => void;
}) {
  const [brief, setBrief] = useState("");
  const [topicId, setTopicId] = useState<number | null>(null);
  const [sentences, setSentences] = useState<Sentence[]>([]);
  const [chorusChoice, setChorusChoice] = useState(""); // "", sentence pl, or "custom"
  const [customChorus, setCustomChorus] = useState("");
  const [style, setStyle] = useState(STYLE_PRESETS[0]);
  const [vocalGender, setVocalGender] = useState("");

  const [draft, setDraft] = useState<SongDraft | null>(null);
  const [lyricsText, setLyricsText] = useState("");
  const [feedback, setFeedback] = useState("");
  const [busy, setBusy] = useState<"draft" | "refine" | "create" | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setChorusChoice("");
    if (topicId === null) {
      setSentences([]);
      return;
    }
    api.browseSentences(topicId).then(setSentences).catch(() => setSentences([]));
  }, [topicId]);

  const chorus = chorusChoice === "custom" ? customChorus.trim() : chorusChoice;
  const songwriter = settings.songwriterModel || settings.aiModel;

  function accept(next: SongDraft) {
    setDraft(next);
    setLyricsText(sectionsToText(next.sections));
  }

  /** The draft as currently edited on screen. */
  function edited(): SongDraft | null {
    if (!draft) return null;
    return { ...draft, sections: textToSections(lyricsText) };
  }

  async function run(kind: "draft" | "refine" | "create", action: () => Promise<void>) {
    setBusy(kind);
    setError(null);
    try {
      await action();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="scroll">
      <div className="row" style={{ marginBottom: 12 }}>
        <button className="icon-btn" onClick={onCancel} aria-label="Back to music">
          <BackIcon />
        </button>
        <h1 className="grow" style={{ marginBottom: 0, fontSize: 22 }}>
          {draft ? "Your song" : "New song"}
        </h1>
      </div>

      {error ? <Banner kind="error">{error}</Banner> : null}

      {!songwriter ? (
        <Banner kind="info">
          Pick a songwriter model in Settings.
        </Banner>
      ) : null}

      {!draft ? (
        <>
          <div className="card">
            <label className="field">
              <span>What should it be about?</span>
              <textarea
                rows={2}
                value={brief}
                placeholder="e.g. ordering breakfast in a café in Kraków"
                onChange={(event) => setBrief(event.target.value)}
              />
            </label>

            <label className="field">
              <span>Words from</span>
              <select
                value={topicId ?? ""}
                onChange={(event) =>
                  setTopicId(event.target.value ? Number(event.target.value) : null)
                }
              >
                <option value="">Words I have practised</option>
                {topics.map((entry) => (
                  <option key={entry.topic.id} value={entry.topic.id}>
                    {entry.topic.titlePl} — {entry.topic.title}
                  </option>
                ))}
              </select>
            </label>

            <label className="field" style={{ marginBottom: chorusChoice === "custom" ? 8 : 0 }}>
              <span>Chorus sentence — the one to get stuck in your head</span>
              <select
                value={chorusChoice}
                onChange={(event) => setChorusChoice(event.target.value)}
              >
                <option value="">Let the songwriter choose</option>
                {sentences.map((sentence) => (
                  <option key={sentence.id} value={sentence.pl}>
                    {sentence.pl} — {sentence.meaning}
                  </option>
                ))}
                <option value="custom">Write my own…</option>
              </select>
            </label>
            {chorusChoice === "custom" ? (
              <input
                value={customChorus}
                placeholder="Polish sentence, e.g. Poproszę kawę z mlekiem"
                onChange={(event) => setCustomChorus(event.target.value)}
              />
            ) : null}
          </div>

          <div className="card">
            <h3>Sound</h3>
            <div className="row wrap" style={{ gap: 6, marginBottom: 10 }}>
              {PRESET_NAMES.map((name, i) => (
                <button
                  key={name}
                  className="chip preset"
                  data-active={style === STYLE_PRESETS[i]}
                  onClick={() => setStyle(STYLE_PRESETS[i])}
                >
                  {name}
                </button>
              ))}
            </div>
            <label className="field">
              <span>Style for Suno</span>
              <input value={style} onChange={(event) => setStyle(event.target.value)} />
            </label>
            <div className="segmented">
              {[
                ["", "Any voice"],
                ["f", "Female"],
                ["m", "Male"],
              ].map(([value, label]) => (
                <button
                  key={value}
                  data-active={vocalGender === value}
                  onClick={() => setVocalGender(value)}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>

          <button
            className="btn primary block"
            disabled={busy !== null || !songwriter || (chorusChoice === "custom" && !chorus)}
            onClick={() =>
              run("draft", async () =>
                accept(
                  await api.songDraft({ brief, topicId, chorusPl: chorus, style, vocalGender }),
                ),
              )
            }
          >
            {busy === "draft" ? (
              <>
                <Spinner /> The songwriter is writing…
              </>
            ) : (
              "✍️ Write the lyrics"
            )}
          </button>
          <div className="row tiny faint" style={{ justifyContent: "center", gap: 6, marginTop: 8 }}>
            {songwriter || "—"}
            <Info title="The songwriter">
              <p>
                An AI model on OpenRouter ({songwriter || "none chosen"}) writes the lyrics
                from your words, leaning on the ones you keep forgetting.
              </p>
              <p>Nothing is sent to Suno until you approve the lyrics.</p>
            </Info>
          </div>
        </>
      ) : (
        <>
          <div className="card">
            <label className="field">
              <span>Title</span>
              <input
                value={draft.title}
                onChange={(event) => setDraft({ ...draft, title: event.target.value })}
              />
            </label>
            <div className="row" style={{ gap: 8 }}>
              <label className="field grow">
                <span>Chorus (Polish)</span>
                <input
                  value={draft.chorusPl}
                  onChange={(event) => setDraft({ ...draft, chorusPl: event.target.value })}
                />
              </label>
              <label className="field grow">
                <span>Meaning</span>
                <input
                  value={draft.chorusEn}
                  onChange={(event) => setDraft({ ...draft, chorusEn: event.target.value })}
                />
              </label>
            </div>
            <label className="field">
              <span>Style for Suno</span>
              <input
                value={draft.style}
                onChange={(event) => setDraft({ ...draft, style: event.target.value })}
              />
            </label>
            <label className="field" style={{ marginBottom: 4 }}>
              <span className="with-info">
                Lyrics
                <Info title="Editing lyrics">
                  <p>One phrase per line: <b>English | Polish</b>.</p>
                  <p>
                    Verses sing the English, then the Polish. The chorus sings only the
                    Polish. Section names go in brackets, like <b>[Chorus]</b>.
                  </p>
                </Info>
              </span>
              <textarea
                className="lyrics-editor"
                rows={16}
                value={lyricsText}
                spellCheck={false}
                onChange={(event) => setLyricsText(event.target.value)}
              />
            </label>
          </div>

          <div className="card">
            <h3>Ask the songwriter for changes</h3>
            <div className="composer">
              <textarea
                rows={2}
                value={feedback}
                placeholder="e.g. shorter verses, more food words, a happier chorus"
                onChange={(event) => setFeedback(event.target.value)}
              />
              <button
                className="btn ghost"
                disabled={busy !== null || feedback.trim().length === 0}
                onClick={() =>
                  run("refine", async () => {
                    const current = edited();
                    if (!current) return;
                    accept(await api.songDraft({ previous: current, feedback }));
                    setFeedback("");
                  })
                }
              >
                {busy === "refine" ? <Spinner /> : "Revise"}
              </button>
            </div>
          </div>

          <button
            className="btn primary block"
            disabled={busy !== null || !settings.hasKieKey}
            onClick={() =>
              run("create", async () => {
                const current = edited();
                if (!current) return;
                await api.songCreate(current);
                onCreated();
              })
            }
          >
            {busy === "create" ? <Spinner /> : `🎤 Make it with Suno ${settings.sunoModel}`}
          </button>
          <div className="row" style={{ gap: 8, marginTop: 8 }}>
            <button className="btn ghost small grow" onClick={() => setDraft(null)}>
              Start over
            </button>
          </div>
          <div className="row tiny faint" style={{ justifyContent: "center", gap: 6, marginTop: 8 }}>
            Uses kie.ai credits
            <Info title="Making the song">
              <p>
                Suno sings two takes, usually in 2–4 minutes. You can leave the screen
                meanwhile.
              </p>
              <p>
                Both takes are downloaded to this device, so they keep playing after kie.ai
                deletes its copy (after 14 days).
              </p>
            </Info>
          </div>
        </>
      )}
    </div>
  );
}
