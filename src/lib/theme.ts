import { api } from "./api";
import { useEffect, useState } from "react";

/**
 * Light/dark appearance. The choice is stored in the database with every
 * other setting, and mirrored in localStorage only because it must be known
 * before the first paint, long before the database has been opened.
 */
export type ThemePreference = "system" | "light" | "dark";
export type Theme = "light" | "dark";

const STORAGE_KEY = "polski.theme";
const media = () => window.matchMedia("(prefers-color-scheme: dark)");

/** Browser chrome / status bar colour per theme; matches --bg in styles.css. */
const CHROME: Record<Theme, string> = { light: "#f6f5fb", dark: "#0c0b16" };

export function readPreference(): ThemePreference {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "light" || stored === "dark" || stored === "system") return stored;
  } catch {
    /* storage unavailable: fall through to the system theme */
  }
  return "system";
}

function resolve(preference: ThemePreference): Theme {
  if (preference !== "system") return preference;
  return media().matches ? "dark" : "light";
}

function apply(preference: ThemePreference) {
  const theme = resolve(preference);
  const root = document.documentElement;
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
  let meta = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
  if (!meta) {
    meta = document.createElement("meta");
    meta.name = "theme-color";
    document.head.appendChild(meta);
  }
  meta.content = CHROME[theme];
}

let current = readPreference();
const listeners = new Set<(preference: ThemePreference) => void>();

/** Applies the saved theme and follows OS changes while set to "system". */
export function initTheme() {
  apply(current);
  media().addEventListener("change", () => {
    if (current === "system") apply(current);
  });
}

/** Adopts the stored choice at startup (it may differ after a restore). */
export function syncThemeFromSettings(stored: "" | ThemePreference) {
  if (stored === "") {
    void api.updateSettings({ theme: current }).catch(() => {});
  } else if (stored !== current) {
    applyPreference(stored);
  }
}

export function setThemePreference(preference: ThemePreference) {
  applyPreference(preference);
  void api.updateSettings({ theme: preference }).catch(() => {});
}

function applyPreference(preference: ThemePreference) {
  current = preference;
  try {
    localStorage.setItem(STORAGE_KEY, preference);
  } catch {
    /* the choice still applies for this run */
  }
  apply(preference);
  listeners.forEach((listener) => listener(preference));
}

export function useThemePreference(): [ThemePreference, (p: ThemePreference) => void] {
  const [preference, setPreference] = useState(current);
  useEffect(() => {
    listeners.add(setPreference);
    return () => {
      listeners.delete(setPreference);
    };
  }, []);
  return [preference, setThemePreference];
}
