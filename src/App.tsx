import { useCallback, useEffect, useState } from "react";

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
import { Progress } from "./screens/Progress";
import { Session } from "./screens/Session";
import { SessionSummary } from "./screens/SessionSummary";
import { SettingsScreen } from "./screens/SettingsScreen";
import { Today } from "./screens/Today";
import { TopicDetail } from "./screens/TopicDetail";
import { Tutor } from "./screens/Tutor";

type Tab = "today" | "tutor" | "progress" | "settings";

const TABS: { id: Tab; glyph: string; label: string }[] = [
  { id: "today", glyph: "🏠", label: "Today" },
  { id: "tutor", glyph: "✨", label: "Tutor" },
  { id: "progress", glyph: "📈", label: "Progress" },
  { id: "settings", glyph: "⚙️", label: "Settings" },
];

/** Either a tab, or a full-screen view that replaces the tab bar. */
type Overlay =
  | { kind: "none" }
  | { kind: "session" }
  | { kind: "summary"; result: SessionResult; xp: number }
  | { kind: "topic"; progress: TopicProgress };

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
      hasVoice: Boolean(settings?.ttsModel),
    });
  }, [settings?.hasApiKey, settings?.ttsModel]);

  /**
   * Warms the audio cache for the session about to start, so every 🔊 tap and
   * every auto-played card is instant instead of waiting on a round trip.
   * Deliberately fire-and-forget: practice must never block on audio.
   */
  const startSession = useCallback(() => {
    stopAudio();
    setOverlay({ kind: "session" });
    if (!plan || !settings?.ttsPrefetch || !settings.ttsModel || !settings.hasApiKey) return;
    const lines = [
      ...plan.dialogue.map((line) => line.pl),
      ...plan.exercises.filter((e) => e.itemKind !== "dialogue").map((e) => e.pl),
    ];
    void api.cacheAudio([...new Set(lines)]).catch(() => {
      /* the 🔊 buttons report per-line failures */
    });
  }, [plan, settings?.ttsPrefetch, settings?.ttsModel, settings?.hasApiKey]);

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
        {TABS.map((entry) => (
          <button
            key={entry.id}
            data-active={tab === entry.id}
            onClick={() => setTab(entry.id)}
          >
            <span className="glyph">{entry.glyph}</span>
            {entry.label}
          </button>
        ))}
      </nav>
    </div>
  );
}
