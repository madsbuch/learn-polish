import { useEffect, useState } from "react";

import {
  CalendarIcon,
  DataIcon,
  DevicesIcon,
  MemoryIcon,
  NavIcon,
  SpeakerIcon,
  StudyIcon,
  ThemeIcon,
} from "../components/icons";
import { List, ListItem, Screen, Section, SplitView, Text, useWideLayout } from "../design";
import { api } from "../lib/api";
import { useThemePreference } from "../lib/theme";
import type { Settings } from "../lib/types";
import { Devices } from "./Devices";
import { HowToUse } from "./HowToUse";
import { Memories } from "./Memories";
import { modelName, sunoLabel, type SettingsPage, type SettingsPageProps } from "./settings/actions";
import { AiSettings } from "./settings/AiSettings";
import { AppearanceSettings } from "./settings/AppearanceSettings";
import { DataSettings } from "./settings/DataSettings";
import { PracticeSettings, reminderTime } from "./settings/PracticeSettings";
import { SongSettings } from "./settings/SongSettings";
import { SoundSettings } from "./settings/SoundSettings";

export type { SettingsPage } from "./settings/actions";

/** What opens beside the menu on a wide window before anything is picked. */
const FIRST_PAGE: SettingsPage = "practice";

interface Shared {
  settings: Settings;
  onSettings: (settings: Settings) => void;
  /** Progress was erased: the screens reload. */
  onReset: () => void;
  /** The downloaded-audio page (under Voices & sound). */
  onOpenAudio: () => void;
}

/**
 * The Settings tab: a short menu, each row opening a sub-screen with that
 * topic's settings. On a phone the sub-screen is pushed over the tabs (App
 * shows it, see SettingsPageView); on a wide window it sits beside the menu.
 */
export function SettingsScreen({
  page,
  onPage,
  ...shared
}: Shared & {
  /** The sub-screen open, if any. */
  page: SettingsPage | null;
  onPage: (page: SettingsPage) => void;
}) {
  const wide = useWideLayout();
  if (!wide) return <SettingsMenu settings={shared.settings} current={null} onPage={onPage} />;
  const current = page ?? FIRST_PAGE;
  return (
    <SplitView
      list={<SettingsMenu settings={shared.settings} current={current} onPage={onPage} />}
      detail={<SettingsPageView key={current} page={current} onOpenPage={onPage} {...shared} />}
    />
  );
}

/**
 * One Settings sub-screen. With `onBack` it is a pushed screen (a phone);
 * without, it is the pane beside the menu.
 */
export function SettingsPageView({
  page,
  ...props
}: Shared & Omit<SettingsPageProps, "settings" | "onSettings"> & { page: SettingsPage }) {
  switch (page) {
    case "howTo":
      return <HowToUse onBack={props.onBack} />;
    case "practice":
      return <PracticeSettings {...props} />;
    case "memories":
      return <Memories onBack={props.onBack} />;
    case "ai":
      return <AiSettings {...props} />;
    case "sound":
      return <SoundSettings {...props} />;
    case "songs":
      return <SongSettings {...props} />;
    case "appearance":
      return <AppearanceSettings onBack={props.onBack} />;
    case "devices":
      return <Devices onBack={props.onBack} />;
    case "data":
      return <DataSettings {...props} />;
  }
}

function SettingsMenu({
  settings,
  current,
  onPage,
}: {
  settings: Settings;
  /** The row whose sub-screen shows beside the menu (wide windows). */
  current: SettingsPage | null;
  onPage: (page: SettingsPage) => void;
}) {
  const [theme] = useThemePreference();
  const [otherDevices, setOtherDevices] = useState<number | null>(null);
  useEffect(() => {
    api
      .syncStatus()
      .then((status) => setOtherDevices(status.devices.filter((device) => !device.isSelf).length))
      .catch(() => setOtherDevices(null));
  }, []);

  const row = (page: SettingsPage, leading: React.ReactNode, title: string, subtitle: string) => (
    <ListItem
      leading={leading}
      title={title}
      subtitle={subtitle}
      selected={current === page}
      onClick={() => onPage(page)}
    />
  );

  const lessons = `${settings.dailyLessons} ${settings.dailyLessons === 1 ? "lesson" : "lessons"} a day`;
  const practice = settings.remindersEnabled
    ? `${lessons} · reminder at ${reminderTime(settings.reminderHour, settings.reminderMinute)}`
    : `${lessons} · no reminder`;
  const ai = !settings.hasApiKey
    ? "Add your OpenRouter key"
    : settings.aiModel
      ? `Connected · ${modelName(settings.aiModel)}`
      : "Connected · pick a tutor model";
  const sound = `${settings.ttsVoiceFemale} and ${settings.ttsVoiceMale} · ${settings.ttsSpeed.toFixed(2)}×`;
  const length = { short: "~2 min", medium: "~3 min", long: "4–5 min" }[settings.songLength];
  const songs = settings.hasKieKey
    ? `kie.ai connected · ${sunoLabel(settings.sunoModel)} · ${length}`
    : "Add a kie.ai key to make songs";
  const appearance = theme === "system" ? "Same as the system" : theme === "dark" ? "Dark" : "Light";
  const devices =
    otherDevices === null
      ? "Keep your phone and computer in step"
      : otherDevices === 0
        ? "Only this one — add your phone or computer"
        : `${otherDevices + 1} devices in step`;

  return (
    <Screen title="Settings">
      <Section title="Getting started">
        <List>
          {row("howTo", <StudyIcon />, "How to use", "The idea behind the app, and how to get the most from it")}
        </List>
      </Section>

      <Section title="Learning">
        <List>
          {row("practice", <CalendarIcon />, "Practice", practice)}
          {row("memories", <MemoryIcon />, "Memories", "What the tutor and examiner remember about you")}
        </List>
      </Section>

      <Section title="AI and sound">
        <List>
          {row("ai", <NavIcon.tutor />, "AI", ai)}
          {row("sound", <SpeakerIcon />, "Voices & sound", sound)}
          {row("songs", <NavIcon.songs />, "Songs", songs)}
        </List>
      </Section>

      <Section title="This app">
        <List>
          {row("appearance", <ThemeIcon />, "Appearance", appearance)}
          {row("devices", <DevicesIcon />, "Devices", devices)}
          {row("data", <DataIcon />, "Your data", "Export, copy keys, diagnostics, reset")}
        </List>
      </Section>

      <Text variant="caption" tone="faint" align="center">
        Polski · learn Polish from English
      </Text>
    </Screen>
  );
}
