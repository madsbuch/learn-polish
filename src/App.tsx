import { useCallback, useEffect, useState, type ReactNode } from "react";

import { Logo, NavIcon } from "./components/icons";
import { Banner, EmptyState, Screen, Spinner, Stack, useWideLayout } from "./design";
import { Startup, type BootStep } from "./components/Startup";
import { SourceReader } from "./components/Sources";
import { api, errorMessage } from "./lib/api";
import { askForNotificationsWhenWorking, onNotificationOpened, syncReminder } from "./lib/notifications";
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
  PracticeContext,
  SessionPlan,
  Settings,
  Stats,
} from "./lib/types";
import { Exercises } from "./screens/Exercises";
import { Progress } from "./screens/Progress";
import { Insights } from "./screens/Insights";
import { AchievementScreen } from "./screens/AchievementScreen";
import { Session } from "./screens/Session";
import { SessionSummary, type SummaryData } from "./screens/SessionSummary";
import { openPage, SettingsPageView, SettingsScreen } from "./screens/SettingsScreen";
import { Search } from "./screens/Search";
import { LessonPage } from "./screens/LessonPage";
import { AudioScreen } from "./screens/AudioScreen";
import { XpDayScreen } from "./screens/XpDayScreen";
import { QuickGames } from "./screens/QuickGames";
import { setHapticsEnabled } from "./lib/haptics";
import { CheckIn, type Writing } from "./screens/CheckIn";
import { Music, preloadLibrary, SongScreen } from "./screens/Music";
import { lessonType, loadLessonTypes } from "./lib/lessonTypes";
import { Player } from "./screens/Player";
import { CourseDetail } from "./screens/CourseDetail";
import { Tutor, TutorChat, tutorAsks } from "./screens/Tutor";
import { onEvent, onSignal } from "./lib/events";
import { FactCheckSchema, TutorDoneSchema } from "./lib/types";
import { setIdOpeners } from "./lib/ids";
import { useSyncReload } from "./lib/sync";
import {
  currentAddress,
  go,
  goPath,
  goTab,
  goUp,
  handoffSlot,
  open,
  startNavigation,
  swapLast,
  up,
  useAddress,
  useEntryKey,
  type Address,
  type Segment,
  type SettingsPage,
  type Tab,
} from "./lib/navigation";

const TABS: { id: Tab; label: string; icon: keyof typeof NavIcon }[] = [
  { id: "exercises", label: "Exercises", icon: "library" },
  { id: "tutor", label: "Tutor", icon: "tutor" },
  { id: "music", label: "Music", icon: "songs" },
  { id: "settings", label: "Settings", icon: "settings" },
];

/** What a summary shows: handed to its page by the drill or conversation that ended. */
const summaries = handoffSlot<SummaryData>();

/** The summary's address: in place of the drill or conversation, above its lesson. */
function summaryAddress(here: Address): Address {
  return swapLast(here, { kind: "done" });
}

/**
 * Where a lesson's practice is logged as reached from (the learning log):
 * the page it was opened from, by its address.
 */
function practiceContext(
  address: Address,
  lessonAt: number,
  lessonId: LessonId,
  courses: CourseProgress[],
  overview: LessonOverview,
): PracticeContext {
  const parent: Segment | undefined = address.trail[lessonAt - 1];
  if (!parent) {
    switch (address.tab) {
      case "exercises":
        // Home lists today's lessons: the daily five.
        return { source: "daily", courseId: overview.due.find((l) => l.id === lessonId)?.courseId ?? null };
      case "tutor":
        return { source: "tutor", courseId: null };
      case "music":
        return { source: "music", courseId: null };
      default:
        return { source: "link", courseId: null };
    }
  }
  switch (parent.kind) {
    case "course": {
      // The course page it was opened from (Other lessons is a course of its own).
      const kind = courses.find((c) => c.course.id === parent.id)?.course.kind;
      return { source: kind === "other" ? "other" : "course", courseId: parent.id };
    }
    case "chat":
      return { source: "tutor", courseId: null };
    case "song":
      return { source: "music", courseId: null };
    case "insight":
    case "progress":
      return { source: "insights", courseId: null };
    default:
      return { source: "link", courseId: null };
  }
}

/** Opens a new tutor conversation, above the page shown, that starts with `ask`. */
function askTutor(ask: string): void {
  open({ kind: "chat", id: "new" }, { handoff: (entry) => tutorAsks.set(entry, ask) });
}

/** What the tutor's buttons and the id chips open, above the page shown. */
const opens = {
  onOpenCourse: (id: CourseId) => open({ kind: "course", id, section: "lessons" }),
  onOpenLesson: (id: LessonId) => open({ kind: "lesson", id }),
  onOpenSong: (id: Parameters<typeof SongScreen>[0]["songId"]) => open({ kind: "song", id }),
  onOpenMemories: () => open({ kind: "settings", page: "memories" }),
};

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
  // What's on screen is the address's to say (lib/navigation.ts).
  const address = useAddress();
  /** A wide window shows a Settings sub-screen beside the menu instead of over it. */
  const wide = useWideLayout();
  const last: Segment | undefined = address.trail[address.trail.length - 1];

  // Studying a lesson — its page, its drill, its check-in — often means pen
  // and paper, not touching the phone: keep the screen on meanwhile.
  const studying = last?.kind === "lesson" || last?.kind === "drill" || last?.kind === "talk";
  useEffect(() => {
    keepScreenAwake(studying, "study");
  }, [studying]);
  const [error, setError] = useState<string | null>(null);

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
          track("Today's lessons", "lessons_overview", () =>
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


  // What an id chip (a title shown in place of an id) opens: above the page
  // it is on, so back comes back to it.
  // Spec: SHELL-17
  useEffect(() => {
    setIdOpeners({ course: opens.onOpenCourse, lesson: opens.onOpenLesson, song: opens.onOpenSong });
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


  // Another device's data arrived (sync, debounced): the screens' data and
  // the settings that came with it (the day's five, the models) are read
  // again. The theme and the other device settings stay this device's own.
  // A drill or check-in that is open carries on with the plan it started
  // with; an open course page takes its fresh progress from the list. The
  // address stays.
  useSyncReload(() => void reloadAll());

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


  // The address is followed from the moment the app has loaded: a reload
  // shows the page it was on, a fresh start the page left within
  // `nav.resume-within`.
  useEffect(() => {
    if (ready) startNavigation();
  }, [ready]);

  // A notification tapped: what it is about.
  // Spec: SHELL-20
  useEffect(() => {
    if (!ready) return;
    const off = onNotificationOpened(goPath);
    return () => {
      void off.then((stop) => stop());
    };
  }, [ready]);

  // A course the address names that isn't in the list: maybe just made (the
  // tutor), so the list is read again; if it is really gone, the page gives
  // way to the one it was opened from.
  // Spec: SHELL-26
  const missingCourse =
    address.trail.find(
      (seg): seg is Extract<Segment, { kind: "course" }> =>
        seg.kind === "course" && !courses.some((c) => c.course.id === seg.id),
    )?.id ?? null;
  useEffect(() => {
    if (!ready || missingCourse === null) return;
    let live = true;
    api
      .getCourseProgress()
      .then((fresh) => {
        if (!live) return;
        setCourses(fresh);
        if (fresh.some((c) => c.course.id === missingCourse)) return;
        const here = currentAddress();
        const at = here.trail.findIndex((seg) => seg.kind === "course" && seg.id === missingCourse);
        if (at !== -1) go({ tab: here.tab, trail: here.trail.slice(0, at) });
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [ready, missingCourse]);

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

  const settingsShared = {
    settings,
    onSettings: applySettings,
    onOpenAudio: () => open({ kind: "audio" }),
  };
  const tutorShared = { ...opens, onChanged: () => void reloadAll() };
  const back = () => {
    goUp();
    void refresh();
  };
  const contextAt = (at: number, lessonId: LessonId) =>
    practiceContext(address, at, lessonId, courses, overview);

  /** The page the address names, full screen; null for a tab's own layout. */
  const page = (): ReactNode => {
    const { tab, trail } = address;
    if (!last) return null;
    const first = trail[0];
    // Pages a tab shows in its own layout, with the tab bar.
    if (tab === "tutor" && trail.length === 1 && first.kind === "chat") return null;
    if (tab === "music" && trail.length === 1 && ["song", "playlist", "compose"].includes(first.kind)) return null;
    if (tab === "music" && trail.length === 2 && first.kind === "song" && last.kind === "source") return null;
    if (tab === "settings" && wide && trail.every((seg) => seg.kind === "settings")) return null;

    const at = trail.length - 1;
    const parent: Segment | undefined = trail[at - 1];
    switch (last.kind) {
      case "course": {
        const progress = courses.find((c) => c.course.id === last.id);
        if (!progress) return <Waiting />;
        return (
          <CourseDetail
            key={last.id}
            progress={progress}
            section={last.section}
            onSection={(section) => go(swapLast(address, { ...last, section }))}
            today={overview.today}
            onBack={goUp}
            onOpenLesson={opens.onOpenLesson}
            onDeleted={() => {
              goUp();
              void reloadAll();
            }}
            onChanged={() => void reloadAll()}
          />
        );
      }
      case "lesson":
        return (
          <LessonPage
            key={last.id}
            lessonId={last.id}
            from={contextAt(at, last.id)}
            today={overview.today}
            onBack={back}
            onStartDrill={(_, challenge) => open({ kind: "drill", challenge })}
            onCheckIn={() => open({ kind: "talk" })}
            onDeleted={back}
            onRead={(source) => open({ kind: "source", id: source.id })}
            onGone={back}
          />
        );
      case "source":
        if (parent?.kind === "lesson") {
          const lessonId = parent.id;
          return (
            <SourceReader
              key={last.id}
              sourceId={last.id}
              load={() => api.lessonSources(lessonId)}
              onBack={goUp}
              onRemoved={goUp}
            />
          );
        }
        if (parent?.kind === "song") {
          const songId = parent.id;
          return <SourceReader key={last.id} sourceId={last.id} load={() => api.songSources(songId)} onBack={goUp} />;
        }
        return null;
      case "drill":
        if (parent?.kind !== "lesson") return null;
        return (
          <DrillPage
            key={parent.id}
            lessonId={parent.id}
            challenge={last.challenge}
            context={contextAt(at - 1, parent.id)}
            prefetchAudio={prefetchAudio}
            onQuit={() => {
              stopAudio();
              back();
            }}
            onFinished={(result, sessionXp, misses) => {
              setStats(result.stats);
              const summary: SummaryData = {
                result,
                xp: sessionXp,
                freezeEarned: result.stats.streakFreezes > stats.streakFreezes,
                title: result.lesson.title,
                graded: result.graded,
                grade: result.grade,
                extra: result.extra,
                score:
                  result.total > 0 ? { correct: result.correct, almost: result.almost, total: result.total } : undefined,
                mastery: result.mastery,
                masteredNow: result.masteredNow,
                impact: result.impact,
                nextDueDay: result.lesson.dueDay,
                misses,
                toReview: [],
              };
              go(summaryAddress(currentAddress()), { handoff: (entry) => summaries.set(entry, summary) });
            }}
            onError={setError}
          />
        );
      case "talk":
        if (parent?.kind !== "lesson") return null;
        return (
          <CheckInPage
            key={parent.id}
            lessonId={parent.id}
            context={contextAt(at - 1, parent.id)}
            onClose={back}
            onFinished={async (first, checkinId, assessment, title) => {
              const done = summaryAddress(currentAddress());
              try {
                // A check-in ends like a drill: on the summary, with what it
                // earned and any achievement it unlocked.
                const result = await api.checkinSummary(checkinId, first);
                setStats(result.stats);
                const summary: SummaryData = {
                  result,
                  xp: assessment.xp,
                  freezeEarned: result.stats.streakFreezes > stats.streakFreezes,
                  title,
                  graded: true,
                  grade: assessment.result === "pass" && assessment.aced ? "ace" : assessment.result,
                  extra: result.mastery?.extra ?? false,
                  mastery: result.mastery,
                  masteredNow: result.masteredNow,
                  impact: assessment.impact ?? null,
                  nextDueDay: result.masteredNow ? null : assessment.nextDueDay,
                  misses: [],
                  toReview: assessment.toReview,
                };
                go(done, { handoff: (entry) => summaries.set(entry, summary) });
              } catch {
                // Back to where the lesson was started from, as Done would.
                go(up(done) ?? done);
              }
              void refresh();
            }}
          />
        );
      case "done":
        return <SummaryPage onDone={back} />;
      case "song":
        return <SongScreen key={last.id} songId={last.id} />;
      case "playing":
        return (
          <Player
            onClose={goUp}
            onOpenSong={(songId) => go({ tab: "music", trail: [{ kind: "song", id: songId }] })}
          />
        );
      case "chat":
        return <TutorChat key={last.id} target={last.id} {...tutorShared} />;
      case "settings":
        return (
          <SettingsPageView key={last.page} page={last.page} onBack={goUp} onOpenPage={openPage} {...settingsShared} />
        );
      case "audio":
      case "audioCourse":
        return (
          <AudioScreen
            courseId={last.kind === "audioCourse" ? last.id : null}
            onBack={goUp}
            onOpenCourse={(id) => open({ kind: "audioCourse", id })}
          />
        );
      case "search":
        return <Search courses={courses} onBack={goUp} />;
      case "progress":
        return (
          <Progress
            stats={stats}
            onBack={goUp}
            onOpenAchievement={(code) => open({ kind: "achievement", code })}
            onOpenDay={(day) => open({ kind: "day", day })}
            onOpenInsights={(start) => open({ kind: "insight", page: start })}
            onAskTutor={askTutor}
          />
        );
      case "insight":
        return (
          <Insights
            page={last.page}
            today={stats.day}
            courses={courses}
            onOpen={(next) => open({ kind: "insight", page: next })}
            onBack={goUp}
            onAskTutor={askTutor}
          />
        );
      case "achievement": {
        const achievement = stats.achievements.find((a) => a.code === last.code);
        return achievement ? (
          <AchievementScreen achievement={achievement} onBack={goUp} />
        ) : (
          <Screen title="Achievement" onBack={goUp} nav={false}>
            <EmptyState title="No such achievement" />
          </Screen>
        );
      }
      case "day":
        return <XpDayScreen day={last.day} onBack={goUp} />;
      case "games":
      case "game":
        return (
          <QuickGames
            playing={last.kind === "game" ? last.game : null}
            onBack={back}
            onPlay={(game) => open({ kind: "game", game })}
            onPlayed={() => void refresh()}
          />
        );
      case "playlist":
      case "compose":
        return null;
    }
  };

  const toastLine = error ? (
    <button className="toast" onClick={() => setError(null)}>
      {error} · tap to dismiss
    </button>
  ) : null;

  const full = page();
  if (full !== null) {
    return (
      <div className="app">
        {full}
        {toastLine}
      </div>
    );
  }

  const tutorOpen = address.tab === "tutor" && address.trail[0]?.kind === "chat" ? address.trail[0].id : null;
  const settingsPages = address.trail.flatMap((seg) => (seg.kind === "settings" ? [seg.page] : []));
  const keyPage: SettingsPage = settings.hasKieKey ? "ai" : "songs";

  return (
    <div className="app app-tabs">
      {address.tab === "exercises" ? (
        <Exercises
          overview={overview}
          stats={stats}
          courses={courses}
          onOpenLesson={(lesson) => opens.onOpenLesson(lesson.id)}
          onOpenCourse={(progress) => opens.onOpenCourse(progress.course.id)}
          onOpenTutor={(ask) => (ask ? askTutor(ask) : goTab("tutor"))}
          onOpenProgress={() => open({ kind: "progress" })}
          onOpenGames={() => open({ kind: "games" })}
          onOpenXpDay={(day) => open({ kind: "day", day })}
          onOpenSearch={() => open({ kind: "search" })}
        />
      ) : null}

      {address.tab === "music" ? (
        <Music
          settings={settings}
          // Songs need the kie.ai key (Settings → Songs); with it, what is
          // missing is the OpenRouter key (Settings → AI).
          onOpenSettings={() => open({ kind: "settings", page: keyPage })}
        />
      ) : null}

      {address.tab === "tutor" ? (
        <Tutor
          open={tutorOpen}
          hasApiKey={settings.hasApiKey}
          hasModel={settings.aiModel.length > 0}
          onOpenSettings={() => open({ kind: "settings", page: "ai" })}
          {...tutorShared}
        />
      ) : null}

      {address.tab === "settings" ? <SettingsScreen pages={settingsPages} {...settingsShared} /> : null}

      {toastLine}

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
              aria-current={address.tab === entry.id ? "page" : undefined}
              // Spec: SHELL-22
              onClick={() => goTab(entry.id)}
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

/** A page whose data is on its way. */
function Waiting() {
  return (
    <Screen onBack={goUp} nav={false}>
      <Spinner />
    </Screen>
  );
}

/**
 * A drill, at its lesson's address (`…/ls_…/drill`): the plan is asked for
 * as it opens, so the page needs nothing handed to it.
 */
function DrillPage({
  lessonId,
  challenge,
  context,
  prefetchAudio,
  onQuit,
  onFinished,
  onError,
}: {
  lessonId: LessonId;
  challenge: boolean;
  context: PracticeContext;
  prefetchAudio: (plan: SessionPlan) => void;
  onQuit: () => void;
  onFinished: Parameters<typeof Session>[0]["onFinished"];
  onError: (message: string) => void;
}) {
  const [plan, setPlan] = useState<SessionPlan | null>(null);
  useEffect(() => {
    let live = true;
    stopAudio();
    api
      .lessonSession(lessonId, context)
      .then((session) => {
        if (!live) return;
        setPlan(session);
        prefetchAudio(session);
      })
      .catch((cause) => {
        if (!live) return;
        onError(errorMessage(cause));
        goUp();
      });
    return () => {
      live = false;
    };
    // Once, for the lesson the page is at.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lessonId]);
  if (!plan) return <Waiting />;
  return <Session plan={plan} challenge={challenge} onQuit={onQuit} onFinished={onFinished} />;
}

/** A conversation lesson's conversation, at its lesson's address (`…/ls_…/talk`). */
function CheckInPage({
  lessonId,
  context,
  onClose,
  onFinished,
}: {
  lessonId: LessonId;
  context: PracticeContext;
  onClose: () => void;
  onFinished: (
    first: boolean,
    checkinId: Parameters<Parameters<typeof CheckIn>[0]["onFinished"]>[1],
    assessment: Parameters<Parameters<typeof CheckIn>[0]["onFinished"]>[2],
    title: string,
  ) => Promise<void>;
}) {
  const [detail, setDetail] = useState<Awaited<ReturnType<typeof api.lessonDetail>> | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    api
      .lessonDetail(lessonId)
      .then((found) => live && setDetail(found))
      .catch(() => live && setDetail(null));
    return () => {
      live = false;
    };
  }, [lessonId]);
  if (detail === undefined) return <Waiting />;
  const title = detail?.title ?? "Conversation";
  // A type with a writing section is played out with a partner.
  const writing: Writing | null =
    detail && lessonType(detail).sections.includes("writing")
      ? { situation: detail.content, goals: detail.objectives, supportLevel: detail.supportLevel }
      : null;
  return (
    <CheckIn
      lessonId={lessonId}
      from={context}
      songId={detail?.songId ?? null}
      writing={writing}
      title={title}
      challenge={detail?.challenge ?? false}
      onClose={onClose}
      onFinished={(first, checkinId, assessment) => onFinished(first, checkinId, assessment, title)}
    />
  );
}

/**
 * The summary, at `…/ls_…/done`: what the drill or conversation handed it.
 * Reopened without that (a reload), it gives way to where the lesson was
 * started from — where its Done goes.
 */
function SummaryPage({ onDone }: { onDone: () => void }) {
  const summary = summaries.get(useEntryKey());
  useEffect(() => {
    if (!summary) goUp();
  }, [summary]);
  if (!summary) return <Waiting />;
  return (
    <SessionSummary
      data={summary}
      onOpenItem={(itemId) => open({ kind: "insight", page: { page: "item", itemId } })}
      onDone={onDone}
    />
  );
}
