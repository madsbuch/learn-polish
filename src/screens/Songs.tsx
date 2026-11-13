import { useCallback, useEffect, useMemo, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { CastPicker } from "../components/CastPicker";
import { FollowLyrics } from "../components/FollowLyrics";
import { PlayerIcon } from "../components/icons";
import { Info, TitleWithInfo } from "../components/Info";
import { Banner, Spinner } from "../components/ui";
import { api, errorMessage } from "../lib/api";
import { sectionLabel, sectionsToText, textToSections } from "../lib/lyrics";
import {
  currentItem,
  cycleRepeat,
  formatTime,
  next,
  playQueue,
  previous,
  seek,
  setCastVolume,
  stopCasting,
  syncLibrary,
  toggle,
  usePlayer,
  type QueueItem,
} from "../lib/player";
import type {
  Sentence,
  Settings,
  Song,
  SongDraft,
  SongSection,
  TopicProgress,
} from "../lib/types";

/** Every downloaded take, in library order, ready to queue. */
function playable(songs: Song[]): QueueItem[] {
  return songs
    .filter((song) => song.status === "ready")
    .flatMap((song) => song.tracks.map((track) => ({ song, track })));
}

export function Songs({
  settings,
  topics,
  onOpenSettings,
}: {
  settings: Settings;
  topics: TopicProgress[];
  onOpenSettings: () => void;
}) {
  const [songs, setSongs] = useState<Song[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [composing, setComposing] = useState(false);
  const [openSong, setOpenSong] = useState<number | null>(null);
  const [picking, setPicking] = useState(false);
  const player = usePlayer();
  const now = currentItem(player);

  const refresh = useCallback(() => {
    api
      .songsList()
      .then((list) => {
        setSongs(list);
        syncLibrary(list);
      })
      .catch((cause) => setError(errorMessage(cause)));
  }, []);

  useEffect(refresh, [refresh]);

  // Songs finish in the background; the backend says when.
  useEffect(() => {
    const unlisten = listen("songs://changed", refresh);
    return () => {
      void unlisten.then((off) => off());
    };
  }, [refresh]);

  const queue = useMemo(() => playable(songs ?? []), [songs]);
  const ready = settings.hasKieKey && settings.hasApiKey;

  if (composing) {
    return (
      <Composer
        settings={settings}
        topics={topics}
        onCancel={() => setComposing(false)}
        onCreated={() => {
          setComposing(false);
          refresh();
        }}
      />
    );
  }

  return (
    <div className="scroll">
      <div className="row between" style={{ marginBottom: 4 }}>
        <TitleWithInfo
          as="h1"
          title="Songs"
          info={
            <>
              <p>
                Short lines in English, then straight into Polish — and a Polish chorus that
                gets stuck in your head. Play them on repeat.
              </p>
              <p>
                Make one with <b>+ New song</b>: pick a theme and a chorus sentence, the
                songwriter drafts the lyrics, and Suno sings two takes. Both are kept on this
                device.
              </p>
            </>
          }
        />
        <button
          className="btn primary small"
          onClick={() => setComposing(true)}
          disabled={!ready}
        >
          + New song
        </button>
      </div>

      {error ? <Banner kind="error">{error}</Banner> : null}

      {!ready ? (
        <Banner kind="info">
          {!settings.hasKieKey && !settings.hasApiKey
            ? "Needs kie.ai and OpenRouter keys."
            : !settings.hasKieKey
              ? "Needs a kie.ai key."
              : "Needs an OpenRouter key."}{" "}
          <button className="link" onClick={onOpenSettings}>
            Open Settings
          </button>
        </Banner>
      ) : null}

      {player.error ? <Banner kind="error">{player.error}</Banner> : null}

      {now ? <NowPlaying item={now} onPickSpeaker={() => setPicking(true)} /> : null}
      {now ? <FollowLyrics item={now} /> : null}

      {queue.length > 0 ? (
        <div className="row" style={{ gap: 8, marginBottom: 14 }}>
          <button className="btn ghost small grow" onClick={() => playQueue(queue, 0)}>
            <PlayerIcon.play size={16} /> Play all
          </button>
          <button
            className="btn ghost small grow"
            onClick={() =>
              playQueue(queue, Math.floor(Math.random() * queue.length), { shuffle: true })
            }
          >
            <PlayerIcon.shuffle /> Shuffle
          </button>
          {!player.cast ? (
            <button
              className="btn ghost small"
              onClick={() => setPicking(true)}
              aria-label="Play on a speaker"
              title="Play on a speaker"
            >
              <PlayerIcon.speaker />
            </button>
          ) : null}
          <button
            className="btn ghost small"
            onClick={cycleRepeat}
            aria-label={`Repeat: ${player.repeat}`}
            title={`Repeat: ${player.repeat}`}
            data-mode={player.repeat}
          >
            <PlayerIcon.repeat />
            {player.repeat === "one" ? "1" : player.repeat === "off" ? "Off" : "All"}
          </button>
        </div>
      ) : null}

      {songs === null ? (
        <div className="center" style={{ padding: 30 }}>
          <Spinner />
        </div>
      ) : songs.length === 0 ? (
        <div className="card center">
          <div style={{ fontSize: 40 }}>🎵</div>
          <h3 style={{ marginBottom: 0 }}>No songs yet</h3>
        </div>
      ) : (
        <div className="card flush">
          {songs.map((song) => (
            <SongRow
              key={song.id}
              song={song}
              open={openSong === song.id}
              playingTrackId={now?.track.id ?? null}
              playing={player.playing}
              onToggleOpen={() => setOpenSong(openSong === song.id ? null : song.id)}
              onPlay={(trackId) => {
                const index = queue.findIndex((item) => item.track.id === trackId);
                if (now?.track.id === trackId) toggle();
                else if (index >= 0) playQueue(queue, index);
              }}
              onChanged={refresh}
              onError={setError}
            />
          ))}
        </div>
      )}

      {picking ? <CastPicker fallbackQueue={queue} onClose={() => setPicking(false)} /> : null}
    </div>
  );
}

// ------------------------------------------------------------------ playing

function Cover({ path, size }: { path: string | null; size: number }) {
  return path ? (
    <img
      className="cover"
      src={convertFileSrc(path)}
      alt=""
      style={{ width: size, height: size }}
    />
  ) : (
    <span className="cover placeholder" style={{ width: size, height: size }}>
      ♪
    </span>
  );
}

function NowPlaying({ item, onPickSpeaker }: { item: QueueItem; onPickSpeaker: () => void }) {
  const player = usePlayer();
  const cast = player.cast;
  const duration = player.duration || item.track.duration;
  return (
    <div className="hero now-playing">
      <div className="row" style={{ alignItems: "flex-start" }}>
        <Cover path={item.track.imagePath} size={72} />
        <div className="grow">
          <div className="eyebrow" style={{ marginBottom: 3 }}>
            {cast ? `On ${cast.device.name}` : "Now playing"} · take {item.track.ord + 1}
          </div>
          <div style={{ fontSize: 18, fontWeight: 750, lineHeight: 1.25 }}>
            {item.song.title}
          </div>
          <div className="selectable" style={{ fontSize: 14, fontWeight: 650, marginTop: 4 }}>
            „{item.song.chorusPl}”
          </div>
          <div style={{ fontSize: 12.5, opacity: 0.75 }}>{item.song.chorusEn}</div>
        </div>
      </div>

      <input
        className="scrubber"
        type="range"
        min={0}
        max={Math.max(1, duration)}
        step={0.5}
        value={Math.min(player.position, duration || player.position)}
        onChange={(event) => seek(Number(event.target.value))}
        aria-label="Position"
      />
      <div className="row between tiny" style={{ opacity: 0.75, marginTop: -2 }}>
        <span>{formatTime(player.position)}</span>
        <span>{formatTime(duration)}</span>
      </div>

      <div className="row transport">
        <button className="icon-btn ghost-light" onClick={previous} aria-label="Previous">
          <PlayerIcon.previous />
        </button>
        <button
          className="play-btn"
          onClick={toggle}
          aria-label={player.playing ? "Pause" : "Play"}
        >
          {player.playing ? <PlayerIcon.pause size={26} /> : <PlayerIcon.play size={26} />}
        </button>
        <button className="icon-btn ghost-light" onClick={next} aria-label="Next">
          <PlayerIcon.next />
        </button>
      </div>

      {cast ? (
        <div className="cast-bar">
          <span aria-hidden="true">🔊</span>
          {cast.detached ? (
            <span className="grow small">{cast.device.name} is playing something else.</span>
          ) : cast.volume >= 0 ? (
            <input
              className="grow"
              type="range"
              min={0}
              max={100}
              value={cast.volume}
              onChange={(event) => setCastVolume(Number(event.target.value))}
              aria-label={`Volume on ${cast.device.name}`}
            />
          ) : (
            <span className="grow small">{cast.device.name}</span>
          )}
          <button className="cast-chip" onClick={() => void stopCasting()}>
            Stop
          </button>
        </div>
      ) : (
        <div className="row" style={{ justifyContent: "center", marginTop: 12 }}>
          <button className="cast-chip" onClick={onPickSpeaker}>
            <PlayerIcon.speaker /> Play on a speaker
          </button>
        </div>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ library

function SongRow({
  song,
  open,
  playingTrackId,
  playing,
  onToggleOpen,
  onPlay,
  onChanged,
  onError,
}: {
  song: Song;
  open: boolean;
  playingTrackId: number | null;
  playing: boolean;
  onToggleOpen: () => void;
  onPlay: (trackId: number) => void;
  onChanged: () => void;
  onError: (message: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const plays = song.tracks.reduce((sum, track) => sum + track.playCount, 0);
  const isCurrent = song.tracks.some((track) => track.id === playingTrackId);

  async function act(action: () => Promise<unknown>) {
    setBusy(true);
    try {
      await action();
      onChanged();
    } catch (cause) {
      onError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="song" data-current={isCurrent}>
      <button className="list-row" onClick={onToggleOpen}>
        <Cover path={song.tracks[0]?.imagePath ?? null} size={48} />
        <span className="grow">
          <span className="pl">{song.title}</span>
          <span className="tr">„{song.chorusPl}”</span>
          <span className="tiny faint" style={{ display: "block", marginTop: 3 }}>
            {song.status === "generating"
              ? "Suno is singing…"
              : song.status === "failed"
                ? "Could not be made"
                : `${song.tracks.length} take${song.tracks.length === 1 ? "" : "s"} · played ${plays}×`}
          </span>
        </span>
        {song.status === "generating" ? <Spinner /> : null}
        {song.status === "failed" ? <span className="chip crimson">Failed</span> : null}
      </button>

      {song.status === "ready" && song.tracks.length > 0 ? (
        <div className="row wrap takes">
          {song.tracks.map((track) => {
            const active = track.id === playingTrackId;
            return (
              <button
                key={track.id}
                className="chip take"
                data-active={active}
                onClick={() => onPlay(track.id)}
              >
                {active && playing ? <PlayerIcon.pause size={13} /> : <PlayerIcon.play size={13} />}
                Take {track.ord + 1} · {formatTime(track.duration)}
              </button>
            );
          })}
        </div>
      ) : null}

      {open ? (
        <div className="song-detail fade-in">
          {song.status === "failed" && song.error ? (
            <Banner kind="error">{song.error}</Banner>
          ) : null}
          <Lyrics sections={song.sections} />
          <p className="tiny faint">Style: {song.style}</p>
          <div className="row wrap" style={{ gap: 8 }}>
            {song.status === "failed" ? (
              <button
                className="btn primary small"
                disabled={busy}
                onClick={() => act(() => api.songRetry(song.id))}
              >
                {busy ? <Spinner /> : "Try again"}
              </button>
            ) : null}
            {song.tracks.length > 1
              ? song.tracks.map((track) => (
                  <button
                    key={track.id}
                    className="btn ghost small"
                    disabled={busy}
                    onClick={() => act(() => api.songTrackDelete(track.id))}
                  >
                    Remove take {track.ord + 1}
                  </button>
                ))
              : null}
            {confirmDelete ? (
              <button
                className="btn danger small"
                disabled={busy}
                onClick={() => act(() => api.songDelete(song.id))}
              >
                Yes, delete the song
              </button>
            ) : (
              <button className="btn danger small" onClick={() => setConfirmDelete(true)}>
                Delete song
              </button>
            )}
          </div>
        </div>
      ) : null}
    </div>
  );
}

function Lyrics({ sections }: { sections: SongSection[] }) {
  return (
    <div className="lyrics selectable">
      {sections.map((section, i) => (
        <div key={i} className="lyric-section" data-kind={section.kind}>
          <div className="eyebrow">{sectionLabel(section.kind)}</div>
          {section.lines.map((line, j) => (
            <div key={j} className="lyric-line">
              {section.kind === "chorus" ? (
                <>
                  <span className="pl">{line.pl}</span>
                  {line.en ? <span className="en">{line.en}</span> : null}
                </>
              ) : (
                <>
                  {line.en ? <span className="en">{line.en}</span> : null}
                  <span className="pl">{line.pl}</span>
                </>
              )}
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

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

function Composer({
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
        <button className="icon-btn" onClick={onCancel} aria-label="Back to songs">
          ‹
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
