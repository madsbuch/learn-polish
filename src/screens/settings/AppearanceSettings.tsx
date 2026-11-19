import { Screen, Section, Segmented } from "../../design";
import { useThemePreference } from "../../lib/theme";
import type { SettingsPageProps } from "./actions";

/** Settings → Appearance: light, dark, or as the system is. */
export function AppearanceSettings({ onBack }: Pick<SettingsPageProps, "onBack">) {
  const [theme, setTheme] = useThemePreference();
  return (
    <Screen title="Appearance" onBack={onBack} nav={false}>
      <Section title="Theme">
        <Segmented
          value={theme}
          onChange={(value) => (value === "system" || value === "light" || value === "dark") && setTheme(value)}
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
        />
      </Section>
    </Screen>
  );
}
