import { useCallback, useEffect, useState } from "react";

import { NavIcon } from "./components/icons";
import { MiniPlayer } from "./components/MiniPlayer";
import { Banner, Loading } from "./components/ui";
import { api, errorMessage } from "./lib/api";
import { syncReminder } from "./lib/notifications";
import { resumeCast, syncRepeatFromSettings } from "./lib/player";
import { syncThemeFromSettings } from "./lib/theme";
import { setSpeechConfig, stop as stopAudio } from "./lib/speech";
import type {
  SessionPlan,
  SessionResult,
  Settings,
  Stats,
  TopicProgress,
} from "./lib/types";
import { Exercises } from "./screens/Exercises";
import { Progress } from "./screens/Progress";
import { Session } from "./screens/Session";
import { SessionSummary } from "./screens/SessionSummary";
import { SettingsScreen } from "./screens/SettingsScreen";
import { Search } from "./screens/Search";
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
  | { kind: "session"; plan: SessionPlan }
  | { kind: "summary"; result: SessionResult; xp: number }
  | { kind: "progress" }
  | { kind: "search" }
  | { kind: "player" }
  | {
      kind: "topic";
      progress: TopicProgress;
      section?: TopicSection;
      /** Where the back button goes. */
      back: Overlay;
    };

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [plan, setPlan] = useState<SessionPlan | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [topics, setTopics] = useState<TopicProgress[]>([]);
  const [tab, setTab] = useState<Tab>("exercises");
  const [overlay, setOverlay] = useState<Overlay>({ kind: "none" });
  const [error, setError] = useState<string | null>(null);
  /** A song the player asked the music tab to show. */
  const [musicSong, setMusicSong] = useState<number | null>(null);
  const clearMusicSong = useCallback(() => setMusicSong(null), []);

  const refresh = useCallback(async () => {
    const [nextPlan, nextStats, nextTopics] = await Promise.all([
      api.getSession(),
      api.getStats(),
      api.getTopicProgress(),
    ]);
    setPlan(nextPlan);
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
        // Re-arm the reminder on every launch: the OS drops scheduled
        // notifications after a reboot or an app update.
        void syncReminder({
          enabled: loaded.remindersEnabled,
          hour: loaded.reminderHour,
          minute: loaded.reminderMinute,
        });
      } catch (cause) {
        if (!cancelled) setError(errorMessage(cause));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [refresh]);

  // Settings can change what today's check-in is (the daily focus).
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
   * Warms the audio cache for a session about to start, so every 🔊 tap and
   * every auto-played card is instant instead of waiting on a round trip.
   * Deliberately fire-and-forget: practice must never block on audio.
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
      void api.cacheAudio(unique).catch(() => {
        /* the 🔊 buttons report per-line failures */
      });
    },
    [settings?.ttsPrefetch, settings?.hasApiKey],
  );

  /** Today's check-in, or — with a topic id — a session for that topic. */
  const startSession = useCallback(
    async (topicId?: number) => {
      stopAudio();
      try {
        const session =
          topicId === undefined || topicId === plan?.topic.id
            ? plan
            : await api.getSession(topicId);
        if (!session) return;
        setOverlay({ kind: "session", plan: session });
        prefetchAudio(session);
      } catch (cause) {
        setError(errorMessage(cause));
      }
    },
    [plan, prefetchAudio],
  );

  /** Something changed behind the screens' backs (the tutor acted). */
  const reloadAll = useCallback(async () => {
    try {
      setSettings(await api.getSettings());
      await refresh();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, [refresh]);

  if (error && !settings) {
    return (
      <div className="center-screen">
        <div>
          <h1>Could not start</h1>
          <Banner kind="error">{error}</Banner>
        </div>
      </div>
    );
  }

  if (!settings || !plan || !stats) {
    return <Loading label="Przygotowuję…" />;
  }

  if (overlay.kind === "session") {
    return (
      <div className="app">
        <Session
          plan={overlay.plan}
          onQuit={() => {
            stopAudio();
            setOverlay({ kind: "none" });
            void refresh();
          }}
          onFinished={(result, sessionXp) => {
            setStats(result.stats);
            setOverlay({ kind: "summary", result, xp: sessionXp });
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
          onDone={() => {
            setOverlay({ kind: "none" });
            setTab("exercises");
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
          isFocus={settings.focusTopicId === overlay.progress.topic.id}
          onBack={() => setOverlay(overlay.back)}
          onPractise={() => void startSession(overlay.progress.topic.id)}
          onFocus={async (focused) => {
            applySettings(await api.setFocus(focused ? overlay.progress.topic.id : null));
          }}
          onDeleted={() => {
            setOverlay(overlay.back);
            void reloadAll();
          }}
        />
      </div>
    );
  }

  if (overlay.kind === "progress") {
    return (
      <div className="app">
        <Progress
          stats={stats}
          topics={topics}
          focusTopicId={settings.focusTopicId}
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
          onClose={() => setOverlay({ kind: "none" })}
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
          plan={plan}
          stats={stats}
          topics={topics}
          focusTopicId={settings.focusTopicId}
          onStart={() => void startSession()}
          onOpenTopic={(progress) => setOverlay({ kind: "topic", progress, back: { kind: "none" } })}
          onOpenTutor={() => setTab("tutor")}
          onOpenProgress={() => setOverlay({ kind: "progress" })}
          onOpenSearch={() => setOverlay({ kind: "search" })}
          onBackToCourse={async () => applySettings(await api.setFocus(null))}
        />
      ) : null}

      {tab === "music" ? (
        <Music
          settings={settings}
          topics={topics}
          openSongId={musicSong}
          onSongOpened={clearMusicSong}
          onOpenSettings={() => setTab("settings")}
        />
      ) : null}

      {tab === "tutor" ? (
        <Tutor
          hasApiKey={settings.hasApiKey}
          hasModel={settings.aiModel.length > 0}
          focusTopicId={settings.focusTopicId}
          onOpenSettings={() => setTab("settings")}
          onPractise={(topicId) => void startSession(topicId)}
          onOpenSong={(songId) => {
            setTab("music");
            setMusicSong(songId);
          }}
          onChanged={() => void reloadAll()}
        />
      ) : null}

      {tab === "settings" ? (
        <SettingsScreen
          settings={settings}
          onSettings={applySettings}
          onReset={() => void refresh()}
        />
      ) : null}

      {error ? (
        <button className="toast" onClick={() => setError(null)}>
          {error} <span className="faint">· dismiss</span>
        </button>
      ) : null}

      <MiniPlayer onOpen={() => setOverlay({ kind: "player" })} />

      <nav className="nav">
        {TABS.map((entry) => {
          const Icon = NavIcon[entry.icon];
          return (
            <button
              key={entry.id}
              data-active={tab === entry.id}
              aria-current={tab === entry.id ? "page" : undefined}
              onClick={() => setTab(entry.id)}
            >
              <span className="glyph">
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
