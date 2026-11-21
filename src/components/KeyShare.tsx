import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Button, List, ListItem, Loading, Section, Sheet, Stack, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { useBackHandler } from "../lib/back";
import { cameraAllowed, canScan, cancelScan, openAppSettings, scanQr } from "../lib/keyshare";
import { toast } from "../lib/toast";
import type { Settings } from "../lib/types";
import { QrIcon, ScanIcon } from "./icons";

/** Copy the API keys to another device: show them as a QR code, or scan one. */
export function KeyShare({ settings, onSettings }: { settings: Settings; onSettings: (settings: Settings) => void }) {
  const [showing, setShowing] = useState(false);
  const [scanning, setScanning] = useState(false);
  const hasKeys = settings.hasApiKey || settings.hasKieKey;
  if (!hasKeys && !canScan) return null;

  const startScan = async () => {
    try {
      if (!(await cameraAllowed())) {
        toast("Polski needs the camera to scan. Allow it in the app's settings.");
        await openAppSettings().catch(() => undefined);
        return;
      }
      setScanning(true);
      const text = await scanQr();
      if (text === null) return;
      const next = await api.keysImport(text);
      const got = [
        next.hasApiKey && !settings.hasApiKey ? "OpenRouter" : null,
        next.hasKieKey && !settings.hasKieKey ? "kie.ai" : null,
      ].filter(Boolean);
      onSettings(next);
      toast(got.length > 0 ? `Saved the ${got.join(" and ")} key.` : "Keys saved.");
    } catch (cause) {
      toast(errorMessage(cause));
    } finally {
      setScanning(false);
    }
  };

  return (
    <Section
      title="Other devices"
      info={
        <>
          <p>
            Copy your API keys to another device without typing them: show the QR code here and scan it with Polski on
            the other device (Settings → Your data → Other devices → Scan).
          </p>
          <p>The QR code holds the keys themselves, so only show it to a device you own.</p>
        </>
      }
    >
      <List>
        {hasKeys ? (
          <ListItem
            leading={<QrIcon />}
            title="Show keys as a QR code"
            subtitle="For another device to scan"
            onClick={() => setShowing(true)}
          />
        ) : null}
        {canScan ? (
          <ListItem
            leading={<ScanIcon />}
            title="Scan keys from another device"
            subtitle="Fills in the OpenRouter and kie.ai keys"
            onClick={() => void startScan()}
          />
        ) : null}
      </List>
      {showing ? <ShowQr onClose={() => setShowing(false)} /> : null}
      {scanning ? <ScanOverlay /> : null}
    </Section>
  );
}

function ShowQr({ onClose }: { onClose: () => void }) {
  const [svg, setSvg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.keysShareQr().then(setSvg, (cause) => setError(errorMessage(cause)));
  }, []);
  return (
    <Sheet title="Your API keys" onClose={onClose}>
      <Stack gap={3}>
        {error ? (
          <Text tone="dim">{error}</Text>
        ) : svg ? (
          <div
            className="key-qr"
            role="img"
            aria-label="QR code with your API keys"
            dangerouslySetInnerHTML={{ __html: svg }}
          />
        ) : (
          <Loading />
        )}
        <Text variant="small" tone="dim" align="center">
          On the other device, open Settings → Your data → Other devices → Scan. Anyone who sees this code can use your keys.
        </Text>
        <Button wide onClick={onClose}>
          Done
        </Button>
      </Stack>
    </Sheet>
  );
}

/** Shown over the camera while it scans: a frame to aim with and a way out. */
export function ScanOverlay() {
  useBackHandler(true, () => void cancelScan());
  return createPortal(
    <div className="scan-overlay">
      <div className="scan-overlay__frame" />
      <div className="scan-overlay__bar">
        <Text tone="dim" align="center">
          Point the camera at the QR code on the other device
        </Text>
        <Button wide onClick={() => void cancelScan()}>
          Cancel
        </Button>
      </div>
    </div>,
    document.body,
  );
}
