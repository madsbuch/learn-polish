import { useCallback, useEffect, useState } from "react";

import { NavIcon } from "./components/icons";
import { Banner, Loading } from "./components/ui";
import { api, errorMessage } from "./lib/api";
import { syncReminder } from "./lib/notifications";
import { setSpeechConfig, stop as stopAudio } from "./lib/speech";
import type {
  SessionPlan,
  SessionResult,
  Settings,
  Stats,
  TopicProgress,
} from "./lib/types";
import { Library } from "./screens/Library";
import { Progress } from "./screens/Progress";
import { Session } from "./screens/Session";
import { SessionSummary } from "./screens/SessionSummary";
import { SettingsScreen } from "./screens/SettingsScreen";
import { Today } from "./screens/Today";
import { TopicDetail, type TopicSection } from "./screens/TopicDetail";
import { Tutor } from "./screens/Tutor";

type Tab = "today" | "library" | "tutor" | "progress" | "settings";

const TABS: { id: Tab; label: string }[] = [
  { id: "today", label: "Today" },
  { id: "library", label: "Library" },
  { id: "tutor", label: "Tutor" },
  { id: "progress", label: "Progress" },
  { id: "settings", label: "Settings" },
];

/** Either a tab, or a full-screen view that replaces the tab bar. */
type Overlay =
  | { kind: "none" }
  | { kind: "session" }
  | { kind: "summary"; result: SessionResult; xp: number }
  | { kind: "topic"; progress: TopicProgress; section?: TopicSection };

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [plan, setPlan] = useState<SessionPlan | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [topics, setTopics] = useState<TopicProgress[]>([]);
  const [tab, setTab] = useState<Tab>("today");
  const [overlay, setOverlay] = useState<Overlay>({ kind: "none" });
  const [error, setError] = useState<string | null>(null);

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

  // Changing the source language re-renders every string from the backend.
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
   * Warms the audio cache for the session about to start, so every 🔊 tap and
   * every auto-played card is instant instead of waiting on a round trip.
   * Deliberately fire-and-forget: practice must never block on audio.
   */
  const startSession = useCallback(() => {
    stopAudio();
    setOverlay({ kind: "session" });
    if (!plan || !settings?.ttsPrefetch || !settings.hasApiKey) return;
    // Dialogue lines carry their speaker's gender so each is warmed in the
    // voice that will actually read it; everything else uses the default.
    const lines = [
      ...plan.dialogue.map((line) => ({ text: line.pl, gender: line.gender })),
      ...plan.exercises
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
  }, [plan, settings?.ttsPrefetch, settings?.hasApiKey]);

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
          plan={plan}
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
            setTab("today");
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
          onBack={() => setOverlay({ kind: "none" })}
        />
      </div>
    );
  }

  return (
    <div className="app">
      {tab === "today" ? (
        <Today
          plan={plan}
          stats={stats}
          onStart={startSession}
          onOpenTutor={() => setTab("tutor")}
        />
      ) : null}

      {tab === "library" ? (
        <Library
          topics={topics}
          onOpenTopic={(progress, section) =>
            setOverlay({ kind: "topic", progress, section })
          }
        />
      ) : null}

      {tab === "tutor" ? (
        <Tutor
          topic={plan.topic}
          hasApiKey={settings.hasApiKey}
          hasModel={settings.aiModel.length > 0}
          onOpenSettings={() => setTab("settings")}
        />
      ) : null}

      {tab === "progress" ? (
        <Progress
          stats={stats}
          topics={topics}
          onOpenTopic={(progress) => setOverlay({ kind: "topic", progress })}
        />
      ) : null}

      {tab === "settings" ? (
        <SettingsScreen
          settings={settings}
          onSettings={applySettings}
          onReset={() => void refresh()}
        />
      ) : null}

      <nav className="nav">
        {TABS.map((entry) => {
          const Icon = NavIcon[entry.id];
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
