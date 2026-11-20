import { useState } from "react";

import { PlayerIcon } from "../../components/icons";
import { Diagnostics } from "../../components/Diagnostics";
import { KeyShare } from "../../components/KeyShare";
import { Button, List, Screen, Section, SettingRow, Switch } from "../../design";
import { api } from "../../lib/api";
import { toast } from "../../lib/toast";
import { useSettingsActions, type SettingsPageProps } from "./actions";

/**
 * Settings → Your data: a copy of the database, the API keys for another
 * device, what the app noted about itself, and — last — starting over.
 */
export function DataSettings({
  settings,
  onSettings,
  onBack,
}: SettingsPageProps) {
  const { busy, run } = useSettingsActions(onSettings);
  const [includeKeys, setIncludeKeys] = useState(false);

  return (
    <Screen
      title="Your data"
      onBack={onBack}
      nav={false}
      info={
        <p>
          Everything the app keeps — progress, courses, the tutor conversations, settings, downloaded audio and your
          songs — lives in one SQLite database on this device. Export saves a copy wherever you choose; it opens in any
          SQLite tool.
        </p>
      }
    >
      <Section title="Export">
        <List>
          <SettingRow
            label="Include API keys"
            description="Off keeps your keys out of the copy"
            control={<Switch label="Include API keys" checked={includeKeys} onChange={setIncludeKeys} />}
          />
        </List>
        <Button
          wide
          icon={<PlayerIcon.download />}
          loading={busy === "export"}
          onClick={() =>
            run("export", async () => {
              const where = await api.exportDatabase(includeKeys);
              if (where) toast(`Exported to ${decodeURIComponent(where)}`);
            })
          }
        >
          Export database
        </Button>
      </Section>

      <KeyShare settings={settings} onSettings={onSettings} />

      <Diagnostics />
    </Screen>
  );
}
