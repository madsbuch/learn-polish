import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { NavIcon } from "./components/icons";
import { Banner, Loading, Stack } from "./design";
import { useBackHandler } from "./lib/back";
import { api, errorMessage } from "./lib/api";
import { syncReminder } from "./lib/notifications";
import { resumeCast, syncRepeatFromSettings } from "./lib/player";
import { syncThemeFromSettings } from "./lib/theme";
import { setSpeechConfig, stop as stopAudio } from "./lib/speech";
import type {
  BlockOverview,
  BlockResult,
  SessionPlan,
  SessionResult,
  Settings,
  Stats,
  TopicProgress,
  TutorDone,
} from "./lib/types";
import { Exercises } from "./screens/Exercises";
import { Progress } from "./screens/Progress";
import { Session } from "./screens/Session";
import { SessionSummary } from "./screens/SessionSummary";
import { Memories } from "./screens/Memories";
import { SettingsScreen } from "./screens/SettingsScreen";
import { Search } from "./screens/Search";
import { BlockPage } from "./screens/BlockPage";
import { AudioScreen } from "./screens/AudioScreen";
import { CheckIn, type Writing } from "./screens/CheckIn";
import { Music } from "./screens/Music";
import { Player } from "./screens/Player";
import { TopicDetail, type TopicSection } from "./screens/TopicDetail";
import { Tutor } from "./screens/Tutor";

type Tab = "exercises" | "tutor" | "music" | "settings";

const TABS: { id: Tab; label: string; icon: keyof typeof NavIcon }[] = [
  { id: "exercises", label: "Exercises", icon: "library" },
  { id: "tutor", label: "Tutor", icon: "tutor" },
  { id: "music", label: "Music", icon: "songs" },
  { id: "settings", label: "Settings", icon: "settings" },
];

/** A full-screen view without the tab bar: practice, or a screen opened from Exercises. */
type Overlay =
  | { kind: "none" }
  | { kind: "block"; blockId: number; back: Overlay }
  | { kind: "session"; plan: SessionPlan; back: Overlay }
  | {
      kind: "checkin";
      blockId: number;
      title: string;
      songId: number | null;
      /** A writing block's situation and goals, shown above the conversation. */
      writing: Writing | null;
      back: Overlay;
    }
  | {
      kind: "summary";
      result: SessionResult;
      xp: number;
      outcome?: { title: string; result: BlockResult; nextDueDay: number | null; extra?: boolean };
      /** Where Done goes: wherever the block was started from. */
      back: Overlay;
    }
  | { kind: "progress" }
  | { kind: "memories"; back: Overlay }
  | { kind: "audio" }
  | { kind: "search" }
  | { kind: "player"; back?: Overlay }
  | {
      kind: "topic";
      progress: TopicProgress;
      section?: TopicSection;
      /** Where the back button goes. */
      back: Overlay;
    };

/**
 * Where to return once a block is done: the screen its page was opened from
 * (a course, or home), not the block page itself.
 */
function startedFrom(back: Overlay): Overlay {
  return back.kind === "block" ? back.back : back;
}

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [overview, setOverview] = useState<BlockOverview | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [topics, setTopics] = useState<TopicProgress[]>([]);
  const [tab, setTab] = useState<Tab>("exercises");
  const [overlay, setOverlay] = useState<Overlay>({ kind: "none" });
  const [error, setError] = useState<string | null>(null);
  /** A song the player asked the music tab to show. */
  const [musicSong, setMusicSong] = useState<number | null>(null);
  const clearMusicSong = useCallback(() => setMusicSong(null), []);
  // Back on another tab goes to Exercises; back on Exercises leaves the app.
  useBackHandler(overlay.kind === "none" && tab !== "exercises", () => setTab("exercises"));

  const refresh = useCallback(async () => {
    const [nextOverview, nextStats, nextTopics] = await Promise.all([
      api.blocksOverview(),
      api.getStats(),
      api.getTopicProgress(),
    ]);
    setOverview(nextOverview);
    setStats(nextStats);
    setTopics(nextTopics);
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const loaded = await api.init();
        if (cancelled) return;
        setSettings(loaded);
        // Display preferences live in the database; adopt them (or seed them).
        syncThemeFromSettings(loaded.theme);
        syncRepeatFromSettings(loaded.playerRepeat);
        // A speaker may still be playing from before a reload.
        resumeCast();
        await refresh();
      } catch (cause) {
        if (!cancelled) setError(errorMessage(cause));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [refresh]);

  const applySettings = useCallback(
    (next: Settings) => {
      setSettings(next);
      void refresh().catch((cause) => setError(errorMessage(cause)));
    },
    [refresh],
  );

  // The speech layer needs to know whether an OpenRouter voice is available,
  // because that decides which of the two engines a 🔊 tap uses.
  useEffect(() => {
    setSpeechConfig({
      hasKey: Boolean(settings?.hasApiKey),
      // Speech is pinned to Gemini, so a voice always exists; only the key
      // decides whether audio is actually reachable.
      hasVoice: true,
    });
  }, [settings?.hasApiKey]);

  /**
   * Downloads the audio a session is about to use, so every 🔊 tap and every
   * auto-played card is instant. The backend does it in the background — in
   * the order the lines will be heard, a few at a time, carrying on with the
   * screen off — and practice never waits for it.
   */
  const prefetchAudio = useCallback(
    (session: SessionPlan) => {
      if (!settings?.ttsPrefetch || !settings.hasApiKey) return;
      // Dialogue lines carry their speaker's gender so each is warmed in the
      // voice that will actually read it; everything else uses the default.
      const lines = [
        ...session.dialogue.map((line) => ({ text: line.pl, gender: line.gender })),
        ...session.exercises
          .filter((e) => e.itemKind !== "dialogue")
          .map((e) => ({ text: e.pl, gender: "" })),
      ];
      const seen = new Set<string>();
      const unique = lines.filter((line) => {
        const key = `${line.gender}\u0001${line.text}`;
        return seen.has(key) ? false : (seen.add(key), true);
      });
      void api.prefetchAudio(unique).catch(() => {
        /* the 🔊 buttons report per-line failures */
      });
    },
    [settings?.ttsPrefetch, settings?.hasApiKey],
  );

  /** Starts a drill block's exercises. */
  const startDrill = useCallback(
    async (blockId: number, back: Overlay) => {
      stopAudio();
      try {
        const session = await api.blockSession(blockId);
        setOverlay({ kind: "session", plan: session, back });
        prefetchAudio(session);
      } catch (cause) {
        setError(errorMessage(cause));
      }
    },
    [prefetchAudio],
  );

  /** Opens a course by id (the tutor's buttons). */
  const openTopicById = useCallback(async (topicId: number) => {
    try {
      const fresh = await api.getTopicProgress();
      setTopics(fresh);
      const progress = fresh.find((entry) => entry.topic.id === topicId);
      if (progress) setOverlay({ kind: "topic", progress, back: { kind: "none" } });
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, []);

  /** Something changed behind the screens' backs (the tutor acted). */
  const reloadAll = useCallback(async () => {
    try {
      setSettings(await api.getSettings());
      await refresh();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, [refresh]);

  // Reminders follow the day: re-armed on every launch (the OS drops them
  // after a reboot or an update), and whenever the day is done or what is
  // due changes, so today's is skipped once there is nothing left to nag about.
  const remindersOn = settings?.remindersEnabled;
  const reminderHour = settings?.reminderHour;
  const reminderMinute = settings?.reminderMinute;
  const doneToday = stats?.completedToday;
  const dueCount = overview?.due.length;
  const streak = stats?.currentStreak;
  useEffect(() => {
    if (remindersOn === undefined || reminderHour === undefined || reminderMinute === undefined) return;
    void syncReminder({
      enabled: remindersOn,
      hour: reminderHour,
      minute: reminderMinute,
      doneToday,
      due: dueCount,
      streak,
    });
  }, [remindersOn, reminderHour, reminderMinute, doneToday, dueCount, streak]);

  // The tutor answers in the background. Off screen, a finished reply is
  // announced with a notification, so the backend needs to know.
  useEffect(() => {
    const report = () => void api.appVisibility(document.visibilityState === "visible").catch(() => {});
    report();
    document.addEventListener("visibilitychange", report);
    return () => document.removeEventListener("visibilitychange", report);
  }, []);

  // A new course's picture arrived (drawn in the background): the lists
  // showing courses reload.
  useEffect(() => {
    const off = listen("pictures://changed", () => void refresh());
    return () => {
      void off.then((stop) => stop());
    };
  }, [refresh]);

  // A background reply that changed things (a course, a song): every screen
  // reloads, wherever the learner is.
  useEffect(() => {
    const off = listen<TutorDone>("tutor://done", (event) => {
      if (event.payload.message && event.payload.message.actions.length > 0) void reloadAll();
    });
    return () => {
      void off.then((stop) => stop());
    };
  }, [reloadAll]);

  if (error && !settings) {
    return (
      <div className="center-screen">
        <Stack gap={4}>
          <h1>Could not start</h1>
          <Banner tone="error">{error}</Banner>
        </Stack>
      </div>
    );
  }

  if (!settings || !overview || !stats) {
    return <Loading label="Przygotowuję…" />;
  }

  if (overlay.kind === "session") {
    return (
      <div className="app">
        <Session
          plan={overlay.plan}
          onQuit={() => {
            stopAudio();
            setOverlay(overlay.back);
            void refresh();
          }}
          onFinished={(result, sessionXp) => {
            setStats(result.stats);
            setOverlay({
              kind: "summary",
              result,
              xp: sessionXp,
              outcome: {
                title: result.block.title,
                result: result.result,
                nextDueDay: result.block.dueDay,
                extra: result.extra,
              },
              back: startedFrom(overlay.back),
            });
          }}
        />
      </div>
    );
  }

  if (overlay.kind === "summary") {
    return (
      <div className="app">
        <SessionSummary
          result={overlay.result}
          sessionXp={overlay.xp}
          outcome={overlay.outcome}
          onDone={() => {
            setOverlay(overlay.back);
            if (overlay.back.kind === "none") setTab("exercises");
            void refresh();
          }}
        />
      </div>
    );
  }

  if (overlay.kind === "topic") {
    return (
      <div className="app">
        <TopicDetail
          progress={overlay.progress}
          initialSection={overlay.section}
          today={overview.today}
          onBack={() => setOverlay(overlay.back)}
          onOpenBlock={(blockId) => setOverlay({ kind: "block", blockId, back: overlay })}
          onDeleted={() => {
            setOverlay(overlay.back);
            void reloadAll();
          }}
          onChanged={() => void reloadAll()}
        />
      </div>
    );
  }

  if (overlay.kind === "block") {
    return (
      <div className="app">
        <BlockPage
          blockId={overlay.blockId}
          today={overview.today}
          onBack={() => {
            setOverlay(overlay.back);
            void refresh();
          }}
          onStartDrill={(blockId) => void startDrill(blockId, overlay)}
          onCheckIn={async (blockId) => {
            const detail = await api.blockDetail(blockId).catch(() => null);
            setOverlay({
              kind: "checkin",
              blockId,
              title: detail?.title ?? "Check-in",
              songId: detail?.songId ?? null,
              writing:
                detail?.format === "writing"
                  ? { situation: detail.lesson, goals: detail.objectives }
                  : null,
              back: overlay,
            });
          }}
          onDeleted={() => {
            setOverlay(overlay.back);
            void refresh();
          }}
        />
      </div>
    );
  }

  if (overlay.kind === "checkin") {
    return (
      <div className="app">
        <CheckIn
          blockId={overlay.blockId}
          songId={overlay.songId}
          writing={overlay.writing}
          title={overlay.title}
          onClose={() => {
            setOverlay(overlay.back);
            void refresh();
          }}
          onFinished={async (first, checkinId, assessment) => {
            const back = startedFrom(overlay.back);
            try {
              // A check-in ends like a drill: on the summary, with what it
              // earned and any achievement it unlocked.
              const result = await api.checkinSummary(checkinId, first);
              setStats(result.stats);
              setOverlay({
                kind: "summary",
                result,
                xp: assessment.xp,
                outcome: { title: overlay.title, result: assessment.result, nextDueDay: assessment.nextDueDay },
                back,
              });
            } catch {
              setOverlay(back);
              if (back.kind === "none") setTab("exercises");
            }
            void refresh();
          }}
        />
      </div>
    );
  }

  if (overlay.kind === "audio") {
    return (
      <div className="app">
        <AudioScreen onBack={() => setOverlay({ kind: "none" })} />
      </div>
    );
  }

  if (overlay.kind === "memories") {
    return (
      <div className="app">
        <Memories onBack={() => setOverlay(overlay.back)} />
      </div>
    );
  }

  if (overlay.kind === "progress") {
    return (
      <div className="app">
        <Progress
          stats={stats}
          topics={topics}
          onBack={() => setOverlay({ kind: "none" })}
          onOpenTopic={(progress) =>
            setOverlay({ kind: "topic", progress, back: { kind: "progress" } })
          }
        />
      </div>
    );
  }

  if (overlay.kind === "player") {
    return (
      <div className="app">
        <Player
          onClose={() => setOverlay(overlay.back ?? { kind: "none" })}
          onOpenSong={(songId) => {
            setOverlay({ kind: "none" });
            setTab("music");
            setMusicSong(songId);
          }}
        />
      </div>
    );
  }

  if (overlay.kind === "search") {
    return (
      <div className="app">
        <Search topics={topics} onBack={() => setOverlay({ kind: "none" })} />
      </div>
    );
  }

  return (
    <div className="app">
      {tab === "exercises" ? (
        <Exercises
          overview={overview}
          stats={stats}
          topics={topics}
          onOpenBlock={(block) => setOverlay({ kind: "block", blockId: block.id, back: { kind: "none" } })}
          onOpenTopic={(progress) => setOverlay({ kind: "topic", progress, back: { kind: "none" } })}
          onOpenTutor={() => setTab("tutor")}
          onOpenProgress={() => setOverlay({ kind: "progress" })}
          onOpenSearch={() => setOverlay({ kind: "search" })}
        />
      ) : null}

      {tab === "music" ? (
        <Music
          settings={settings}
          topics={topics}
          openSongId={musicSong}
          onSongOpened={clearMusicSong}
          onOpenBlock={(blockId) => setOverlay({ kind: "block", blockId, back: { kind: "none" } })}
          onOpenSettings={() => setTab("settings")}
          onOpenPlayer={() => setOverlay({ kind: "player", back: { kind: "none" } })}
        />
      ) : null}

      {tab === "tutor" ? (
        <Tutor
          hasApiKey={settings.hasApiKey}
          hasModel={settings.aiModel.length > 0}
          onOpenSettings={() => setTab("settings")}
          onOpenTopic={(topicId) => void openTopicById(topicId)}
          onOpenBlock={(blockId) => setOverlay({ kind: "block", blockId, back: { kind: "none" } })}
          onOpenSong={(songId) => {
            setTab("music");
            setMusicSong(songId);
          }}
          onChanged={() => void reloadAll()}
          onOpenMemories={() => setOverlay({ kind: "memories", back: { kind: "none" } })}
        />
      ) : null}

      {tab === "settings" ? (
        <SettingsScreen
          settings={settings}
          onSettings={applySettings}
          onReset={() => void refresh()}
          onOpenMemories={() => setOverlay({ kind: "memories", back: { kind: "none" } })}
          onOpenAudio={() => setOverlay({ kind: "audio" })}
        />
      ) : null}

      {error ? (
        <button className="toast" onClick={() => setError(null)}>
          {error} · tap to dismiss
        </button>
      ) : null}


      <nav className="nav">
        {TABS.map((entry) => {
          const Icon = NavIcon[entry.icon];
          return (
            <button
              key={entry.id}
              aria-current={tab === entry.id ? "page" : undefined}
              onClick={() => setTab(entry.id)}
            >
              <span className="nav-glyph">
                <Icon />
              </span>
              {entry.label}
            </button>
          );
        })}
      </nav>
    </div>
  );
}
