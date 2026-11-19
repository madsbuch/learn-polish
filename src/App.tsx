import { useCallback, useEffect, useState } from "react";

import { Logo, NavIcon } from "./components/icons";
import { Banner, markBack, Stack, useWideLayout } from "./design";
import { Startup, type BootStep } from "./components/Startup";
import { useBackHandler } from "./lib/back";
import { api, errorMessage } from "./lib/api";
import { askForNotificationsWhenWorking, syncReminder } from "./lib/notifications";
import { toast } from "./lib/toast";
import {
  keepScreenAwake,
  resumeCast,
  syncRepeatFromSettings,
} from "./lib/player";
import { syncThemeFromSettings } from "./lib/theme";
import { setSpeechConfig, stop as stopAudio } from "./lib/speech";
import type {
  CourseId,
  CourseProgress,
  LessonId,
  LessonOverview,
  LessonResult,
  SessionPlan,
  SessionResult,
  Settings,
  SongId,
  Stats,
} from "./lib/types";
import { Exercises } from "./screens/Exercises";
import { Progress } from "./screens/Progress";
import { Insights, type InsightPage } from "./screens/Insights";
import { AchievementScreen } from "./screens/AchievementScreen";
import { Session } from "./screens/Session";
import { SessionSummary } from "./screens/SessionSummary";
import { Memories } from "./screens/Memories";
import { SettingsPageView, SettingsScreen, type SettingsPage } from "./screens/SettingsScreen";
import { Search } from "./screens/Search";
import { LessonPage } from "./screens/LessonPage";
import { AudioScreen } from "./screens/AudioScreen";
import { XpDayScreen } from "./screens/XpDayScreen";
import { QuickGames } from "./screens/QuickGames";
import { setHapticsEnabled } from "./lib/haptics";
import { CheckIn, type Writing } from "./screens/CheckIn";
import { Music, preloadLibrary } from "./screens/Music";
import { lessonType, loadLessonTypes } from "./lib/lessonTypes";
import { Player } from "./screens/Player";
import { CourseDetail, type CourseSection } from "./screens/CourseDetail";
import { Tutor } from "./screens/Tutor";
import { onEvent, onSignal } from "./lib/events";
import { FactCheckSchema, TutorDoneSchema } from "./lib/types";
import { setIdOpeners } from "./lib/ids";
import { useSyncReload } from "./lib/sync";

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
  | { kind: "lesson"; lessonId: LessonId; back: Overlay }
  | { kind: "session"; plan: SessionPlan; back: Overlay }
  | {
      kind: "checkin";
      lessonId: LessonId;
      title: string;
      songId: SongId | null;
      /** A writing lesson's situation and goals, shown above the conversation. */
      writing: Writing | null;
      back: Overlay;
    }
  | {
      kind: "summary";
      result: SessionResult;
      xp: number;
      /** The lesson took the learner up a level (the summary celebrates). */
      levelUp?: boolean;
      outcome?: {
        title: string;
        result: LessonResult;
        nextDueDay: number | null;
        extra?: boolean;
        /** A drill's score; a check-in has none. */
        score?: { correct: number; total: number };
      };
      /** Where Done goes: wherever the lesson was started from. */
      back: Overlay;
    }
  | { kind: "progress" }
  | { kind: "insights"; start: InsightPage }
  /** Quick games, from Home (screens/QuickGames.tsx). */
  | { kind: "games" }
  | { kind: "achievement"; code: string }
  | { kind: "xpDay"; day: number; back: Overlay }
  | { kind: "memories"; back: Overlay }
  | { kind: "audio" }
  | { kind: "search" }
  | { kind: "player"; back?: Overlay }
  | {
      kind: "course";
      progress: CourseProgress;
      section?: CourseSection;
      /** Where the back button goes. */
      back: Overlay;
    };

/**
 * Where to return once a lesson is done: the screen its page was opened from
 * (a course, or home), not the lesson page itself.
 */
function startedFrom(back: Overlay): Overlay {
  return back.kind === "lesson" ? back.back : back;
}

/**
 * `init_app`, asked again if the first call has not answered in a few
 * seconds: on Android a call made while the webview is still starting can
 * go unanswered, and a second one gets through. It is safe to repeat; the
 * first answer wins.
 */
function initApp(): Promise<Settings> {
  return new Promise((resolve, reject) => {
    let settled = false;
    const settle = (done: () => void) => {
      if (settled) return;
      settled = true;
      window.clearTimeout(again);
      done();
    };
    const attempt = () =>
      api.init().then(
        (value) => settle(() => resolve(value)),
        (cause) => settle(() => reject(cause)),
      );
    const again = window.setTimeout(() => {
      if (!settled) void attempt();
    }, 6000);
    void attempt();
  });
}

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [overview, setOverview] = useState<LessonOverview | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [courses, setCourses] = useState<CourseProgress[]>([]);
  const [tab, setTab] = useState<Tab>("exercises");
  /** A request for the tutor from a button elsewhere ("Add levels"): starts a new conversation. */
  const [tutorAsk, setTutorAsk] = useState<string | null>(null);
  const [overlay, setOverlay] = useState<Overlay>({ kind: "none" });
  /**
   * The Settings sub-screen open (How to use, AI, …). It stays while a
   * screen opened from it (Downloaded audio) is on top, so back returns to it.
   */
  const [settingsPage, setSettingsPage] = useState<SettingsPage | null>(null);
  /** A wide window shows a Settings sub-screen beside the menu instead of over it. */
  const wide = useWideLayout();
  /** Opens Settings at one of its sub-screens (a "set your key" button elsewhere). */
  const openSettings = useCallback((page: SettingsPage) => {
    setOverlay({ kind: "none" });
    setTab("settings");
    setSettingsPage(page);
  }, []);

  // Studying a lesson — its page, its drill, its check-in — often means pen
  // and paper, not touching the phone: keep the screen on meanwhile.
  const studying =
    overlay.kind === "lesson" ||
    overlay.kind === "session" ||
    overlay.kind === "checkin";
  useEffect(() => {
    keepScreenAwake(studying, "study");
  }, [studying]);
  const [error, setError] = useState<string | null>(null);
  /** A song the player asked the music tab to show. */
  const [musicSong, setMusicSong] = useState<SongId | null>(null);
  const clearMusicSong = useCallback(() => setMusicSong(null), []);
  // Back on another tab goes to Exercises; back on Exercises leaves the app.
  useBackHandler(overlay.kind === "none" && tab !== "exercises", () =>
    setTab("exercises"),
  );

  const refresh = useCallback(async () => {
    const [nextOverview, nextStats, nextCourses] = await Promise.all([
      api.lessonsOverview(),
      api.getStats(),
      api.getCourseProgress(),
    ]);
    setOverview(nextOverview);
    setStats(nextStats);
    setCourses(nextCourses);
  }, []);

  // Starting up, step by step, shown on the loading screen so a stall says
  // where it stalled.
  const [bootSteps, setBootSteps] = useState<BootStep[]>([]);
  const [bootRun, setBootRun] = useState(0);
  useEffect(() => {
    let cancelled = false;
    const track = async <T,>(
      label: string,
      command: string,
      run: () => Promise<T>,
    ): Promise<T> => {
      const startedAt = Date.now();
      setBootSteps((steps) => [
        ...steps.filter((s) => s.command !== command),
        { label, command, state: "pending", startedAt },
      ]);
      const settle = (patch: Partial<BootStep>) =>
        !cancelled &&
        setBootSteps((steps) =>
          steps.map((s) =>
            s.command === command
              ? { ...s, ...patch, ms: Date.now() - startedAt }
              : s,
          ),
        );
      try {
        const value = await run();
        settle({ state: "done" });
        return value;
      } catch (cause) {
        settle({ state: "failed", error: errorMessage(cause) });
        throw cause;
      }
    };
    setBootSteps([]);
    (async () => {
      try {
        const loaded = await track("Opening your data", "init_app", initApp);
        // The lesson types, which every lesson list and page reads.
        await track("Loading lesson types", "lesson_types", loadLessonTypes);
        if (cancelled) return;
        setSettings(loaded);
        // Display preferences live in the database; adopt them (or seed them).
        syncThemeFromSettings(loaded.theme);
        syncRepeatFromSettings(loaded.playerRepeat);
        // A speaker may still be playing from before a reload.
        resumeCast();
        const [nextOverview, nextStats, nextCourses] = await Promise.all([
          track("Today's check-ins", "lessons_overview", () =>
            api.lessonsOverview(),
          ),
          track("Your stats", "get_stats", () => api.getStats()),
          track("Your courses", "get_course_progress", () =>
            api.getCourseProgress(),
          ),
        ]);
        if (cancelled) return;
        setOverview(nextOverview);
        setStats(nextStats);
        setCourses(nextCourses);
      } catch (cause) {
        if (!cancelled) setError(errorMessage(cause));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [bootRun]);

  const applySettings = useCallback(
    (next: Settings) => {
      setSettings(next);
      void refresh().catch((cause) => setError(errorMessage(cause)));
    },
    [refresh],
  );

  // Answers are felt on a phone unless this device's setting says not.
  useEffect(() => {
    setHapticsEnabled(settings?.haptics ?? true);
  }, [settings?.haptics]);

  // The speech layer needs to know whether an OpenRouter voice is available,
  // because that decides which of the two engines a 🔊 tap uses.
  useEffect(() => {
    setSpeechConfig({
      hasKey: Boolean(settings?.hasApiKey),
      // Speech is pinned to Gemini, so a voice always exists; only the key
      // decides whether audio is actually reachable.
      hasVoice: true,
      speed: settings?.ttsSpeed,
    });
  }, [settings?.hasApiKey, settings?.ttsSpeed]);

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
        ...session.dialogue.map((line) => ({
          text: line.pl,
          gender: line.gender,
        })),
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

  /** Starts a drill lesson's exercises. */
  const startDrill = useCallback(
    async (lessonId: LessonId, back: Overlay) => {
      stopAudio();
      try {
        const session = await api.lessonSession(lessonId);
        setOverlay({ kind: "session", plan: session, back });
        prefetchAudio(session);
      } catch (cause) {
        setError(errorMessage(cause));
      }
    },
    [prefetchAudio],
  );

  /** Opens a course by id (the tutor's buttons). */
  const openCourseById = useCallback(async (courseId: CourseId) => {
    try {
      const fresh = await api.getCourseProgress();
      setCourses(fresh);
      const progress = fresh.find((entry) => entry.course.id === courseId);
      if (progress)
        setOverlay({ kind: "course", progress, back: { kind: "none" } });
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, []);

  // What an id chip (a title shown in place of an id) opens.
  useEffect(() => {
    setIdOpeners({
      course: (courseId) => void openCourseById(courseId),
      lesson: (lessonId) =>
        setOverlay({ kind: "lesson", lessonId, back: { kind: "none" } }),
      song: (songId) => {
        setOverlay({ kind: "none" });
        setTab("music");
        setMusicSong(songId);
      },
    });
  }, [openCourseById]);

  /** Something changed behind the screens' backs (the tutor acted). */
  const reloadAll = useCallback(async () => {
    try {
      setSettings(await api.getSettings());
      await refresh();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, [refresh]);

  // Another device's data arrived (sync, debounced): the screens' data and
  // the settings that came with it (the day's five, the models) are read
  // again. The theme and the other device settings stay this device's own.
  // A drill or check-in that is open carries on with the plan it started
  // with; an open course page takes its fresh progress.
  useSyncReload(() => {
    void (async () => {
      await reloadAll();
      const fresh = await api.getCourseProgress().catch(() => null);
      if (!fresh) return;
      setOverlay((current) => {
        if (current.kind !== "course") return current;
        const progress = fresh.find((entry) => entry.course.id === current.progress.course.id);
        return progress ? { ...current, progress } : current;
      });
    })();
  });

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
    if (
      remindersOn === undefined ||
      reminderHour === undefined ||
      reminderMinute === undefined
    )
      return;
    void syncReminder({
      enabled: remindersOn,
      hour: reminderHour,
      minute: reminderMinute,
      doneToday,
      due: dueCount,
      streak,
    });
  }, [remindersOn, reminderHour, reminderMinute, doneToday, dueCount, streak]);

  // The music library, loaded once the app is up, so Music opens at once.
  const ready = settings !== null;
  useEffect(() => {
    if (!ready) return;
    const timer = window.setTimeout(preloadLibrary, 1500);
    return () => window.clearTimeout(timer);
  }, [ready]);

  // The tutor answers in the background. Off screen, a finished reply is
  // announced with a notification, so the backend needs to know.
  useEffect(() => {
    const report = () =>
      void api
        .appVisibility(document.visibilityState === "visible")
        .catch(() => {});
    report();
    document.addEventListener("visibilitychange", report);
    return () => document.removeEventListener("visibilitychange", report);
  }, []);

  // A lesson corrected against Wikipedia in the background (the
  // three-monthly check at start).
  useEffect(() => {
    const off = onEvent(FactCheckSchema, "facts://updated", (event) => {
      toast(`“${event.payload.title}” was updated from Wikipedia`, "info");
    });
    return () => {
      void off.then((stop) => stop());
    };
  }, []);

  // A new course's picture arrived (drawn in the background): the lists
  // showing courses reload.
  useEffect(() => {
    const off = onSignal("pictures://changed", () => void refresh());
    return () => {
      void off.then((stop) => stop());
    };
  }, [refresh]);

  // Work in the background: notifications, so its notification can show.
  useEffect(() => {
    const off = askForNotificationsWhenWorking();
    return () => {
      void off.then((stop) => stop());
    };
  }, []);

  // A background reply that changed things (a course, a song): every screen
  // reloads, wherever the learner is.
  useEffect(() => {
    const off = onEvent(TutorDoneSchema, "tutor://done", (event) => {
      if (event.payload.message && event.payload.message.actions.length > 0)
        void reloadAll();
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
    return (
      <Startup
        steps={bootSteps}
        onRetry={() => {
          setError(null);
          setBootRun((n) => n + 1);
        }}
      />
    );
  }

  if (overlay.kind === "session") {
    return (
      <div className="app">
        <Session
          plan={overlay.plan}
          onQuit={() => {
            stopAudio();
            markBack();
            setOverlay(overlay.back);
            void refresh();
          }}
          onFinished={(result, sessionXp) => {
            setStats(result.stats);
            setOverlay({
              kind: "summary",
              result,
              xp: sessionXp,
              levelUp: result.stats.level > stats.level,
              outcome: {
                title: result.lesson.title,
                result: result.result,
                nextDueDay: result.lesson.dueDay,
                extra: result.extra,
                score:
                  result.total > 0
                    ? { correct: result.correct, total: result.total }
                    : undefined,
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
          levelUp={overlay.levelUp}
          outcome={overlay.outcome}
          onDone={() => {
            // Back to where the lesson was started: a return, not a new screen.
            markBack();
            setOverlay(overlay.back);
            if (overlay.back.kind === "none") setTab("exercises");
            void refresh();
          }}
        />
      </div>
    );
  }

  if (overlay.kind === "course") {
    return (
      <div className="app">
        <CourseDetail
          progress={overlay.progress}
          initialSection={overlay.section}
          today={overview.today}
          onBack={() => setOverlay(overlay.back)}
          onOpenLesson={(lessonId) =>
            setOverlay({ kind: "lesson", lessonId, back: overlay })
          }
          onDeleted={() => {
            setOverlay(overlay.back);
            void reloadAll();
          }}
          onChanged={() => void reloadAll()}
        />
      </div>
    );
  }

  if (overlay.kind === "lesson") {
    return (
      <div className="app">
        <LessonPage
          lessonId={overlay.lessonId}
          today={overview.today}
          onBack={() => {
            setOverlay(overlay.back);
            void refresh();
          }}
          onStartDrill={(lessonId) => void startDrill(lessonId, overlay)}
          onCheckIn={async (lessonId) => {
            const detail = await api.lessonDetail(lessonId).catch(() => null);
            setOverlay({
              kind: "checkin",
              lessonId,
              title: detail?.title ?? "Conversation",
              songId: detail?.songId ?? null,
              writing:
                // A type with a writing section is played out with a partner.
                detail && lessonType(detail).sections.includes("writing")
                  ? {
                      situation: detail.content,
                      goals: detail.objectives,
                      supportLevel: detail.supportLevel,
                    }
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
          lessonId={overlay.lessonId}
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
                levelUp: result.stats.level > stats.level,
                outcome: {
                  title: overlay.title,
                  result: assessment.result,
                  nextDueDay: assessment.nextDueDay,
                },
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

  if (overlay.kind === "xpDay") {
    return (
      <div className="app">
        <XpDayScreen
          day={overlay.day}
          onBack={() => setOverlay(overlay.back)}
        />
      </div>
    );
  }

  if (overlay.kind === "games") {
    return (
      <div className="app">
        <QuickGames
          level={stats.level}
          onBack={() => {
            setOverlay({ kind: "none" });
            void refresh();
          }}
          onPlayed={() => void refresh()}
        />
      </div>
    );
  }

  if (overlay.kind === "progress") {
    return (
      <div className="app">
        <Progress
          stats={stats}
          onBack={() => setOverlay({ kind: "none" })}
          onOpenAchievement={(code) => setOverlay({ kind: "achievement", code })}
          onOpenDay={(day) =>
            setOverlay({ kind: "xpDay", day, back: { kind: "progress" } })
          }
          onOpenInsights={(start) => setOverlay({ kind: "insights", start })}
          onAskTutor={(ask) => {
            setOverlay({ kind: "none" });
            setTutorAsk(ask);
            setTab("tutor");
          }}
        />
      </div>
    );
  }

  if (overlay.kind === "insights") {
    return (
      <div className="app">
        <Insights
          start={overlay.start}
          today={stats.day}
          courses={courses}
          onDone={() => setOverlay({ kind: "progress" })}
          onAskTutor={(ask) => {
            setOverlay({ kind: "none" });
            setTutorAsk(ask);
            setTab("tutor");
          }}
        />
      </div>
    );
  }

  if (overlay.kind === "achievement") {
    const achievement = stats.achievements.find((a) => a.code === overlay.code);
    if (achievement) {
      return (
        <div className="app">
          <AchievementScreen achievement={achievement} onBack={() => setOverlay({ kind: "progress" })} />
        </div>
      );
    }
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
        <Search courses={courses} onBack={() => setOverlay({ kind: "none" })} />
      </div>
    );
  }

  const settingsShared = {
    settings,
    onSettings: applySettings,
    onReset: () => void refresh(),
    onOpenAudio: () => setOverlay({ kind: "audio" }),
  };

  // A Settings sub-screen on a phone: pushed over the tabs, back to the menu.
  if (tab === "settings" && settingsPage !== null && !wide) {
    return (
      <div className="app">
        <SettingsPageView
          key={settingsPage}
          page={settingsPage}
          onBack={() => setSettingsPage(null)}
          onOpenPage={setSettingsPage}
          {...settingsShared}
        />
      </div>
    );
  }

  return (
    <div className="app app-tabs">
      {tab === "exercises" ? (
        <Exercises
          overview={overview}
          stats={stats}
          courses={courses}
          onOpenLesson={(lesson) =>
            setOverlay({
              kind: "lesson",
              lessonId: lesson.id,
              back: { kind: "none" },
            })
          }
          onOpenCourse={(progress) =>
            setOverlay({ kind: "course", progress, back: { kind: "none" } })
          }
          onOpenTutor={(ask) => {
            setTutorAsk(ask ?? null);
            setTab("tutor");
          }}
          onOpenProgress={() => setOverlay({ kind: "progress" })}
          onOpenGames={() => setOverlay({ kind: "games" })}
          onOpenXpDay={(day) =>
            setOverlay({ kind: "xpDay", day, back: { kind: "none" } })
          }
          onOpenSearch={() => setOverlay({ kind: "search" })}
        />
      ) : null}

      {tab === "music" ? (
        <Music
          settings={settings}
          openSongId={musicSong}
          onSongOpened={clearMusicSong}
          onOpenLesson={(lessonId) =>
            setOverlay({ kind: "lesson", lessonId, back: { kind: "none" } })
          }
          // Songs need the kie.ai key (Settings → Songs); with it, what is
          // missing is the OpenRouter key (Settings → AI).
          onOpenSettings={() => openSettings(settings.hasKieKey ? "ai" : "songs")}
          onOpenPlayer={() =>
            setOverlay({ kind: "player", back: { kind: "none" } })
          }
        />
      ) : null}

      {tab === "tutor" ? (
        <Tutor
          hasApiKey={settings.hasApiKey}
          hasModel={settings.aiModel.length > 0}
          onOpenSettings={() => openSettings("ai")}
          onOpenCourse={(courseId) => void openCourseById(courseId)}
          onOpenLesson={(lessonId) =>
            setOverlay({ kind: "lesson", lessonId, back: { kind: "none" } })
          }
          onOpenSong={(songId) => {
            setTab("music");
            setMusicSong(songId);
          }}
          onChanged={() => void reloadAll()}
          onOpenMemories={() =>
            setOverlay({ kind: "memories", back: { kind: "none" } })
          }
          ask={tutorAsk}
          onAsked={() => setTutorAsk(null)}
        />
      ) : null}

      {tab === "settings" ? (
        <SettingsScreen
          page={settingsPage}
          onPage={setSettingsPage}
          {...settingsShared}
        />
      ) : null}

      {error ? (
        <button className="toast" onClick={() => setError(null)}>
          {error} · tap to dismiss
        </button>
      ) : null}

      <nav className="nav" aria-label="Main navigation">
        <div className="nav-brand" aria-hidden="true">
          <Logo size={40} />
          <span>Polski</span>
        </div>
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
