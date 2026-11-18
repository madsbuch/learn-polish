import { useCallback, useEffect, useRef, useState } from "react";

import { copyText } from "../components/Diagnostics";
import { DevicesIcon, QrIcon, ScanIcon } from "../components/icons";
import { ScanOverlay } from "../components/KeyShare";
import {
  Badge,
  Banner,
  Button,
  ConfirmButton,
  Field,
  Inline,
  List,
  ListEmpty,
  ListItem,
  Loading,
  Screen,
  Section,
  Sheet,
  Spinner,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { onSyncStatus } from "../lib/events";
import { cameraAllowed, canScan, openAppSettings, scanQr } from "../lib/keyshare";
import { ago, describeHeld, syncErrorText, useSyncReload } from "../lib/sync";
import { toast } from "../lib/toast";
import type { JoinResult, SyncDevice, SyncOffer, SyncStatus } from "../lib/types";

const PLATFORMS: Record<string, string> = {
  android: "Android",
  ios: "iPhone",
  macos: "Mac",
  windows: "Windows",
  linux: "Linux",
};

function platformName(platform: string): string {
  return PLATFORMS[platform] ?? platform;
}

/** "Synced 5 min ago · up to date", for a device in the list. */
function deviceLine(device: SyncDevice, now: number): string {
  const parts = [platformName(device.platform)];
  if (device.lastSyncAt) {
    parts.push(`synced ${ago(device.lastSyncAt, now)}`);
    parts.push(device.complete ? "up to date" : "still catching up");
  } else {
    parts.push("not synced yet");
  }
  return parts.join(" · ");
}

/** The newest sync with any other device. */
function lastSynced(status: SyncStatus): string | null {
  const times = status.devices
    .filter((device) => !device.isSelf && device.lastSyncAt)
    .map((device) => device.lastSyncAt ?? "");
  return times.length > 0 ? times.reduce((a, b) => (Date.parse(a) > Date.parse(b) ? a : b)) : null;
}

/**
 * Settings → Devices: the learner's devices kept in step (docs/SYNC.md).
 * Shows this device and the others, adds one (a QR code to scan) or joins
 * another's, and syncs on request.
 */
export function Devices({ onBack }: { onBack: () => void }) {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sheet, setSheet] = useState<"rename" | "offer" | "join" | null>(null);
  const [device, setDevice] = useState<SyncDevice | null>(null);
  const [now, setNow] = useState(() => Date.now());

  const refresh = useCallback(() => {
    api.syncStatus().then(
      (next) => {
        setStatus(next);
        setError(null);
      },
      (cause) => setError(errorMessage(cause)),
    );
  }, []);

  useEffect(refresh, [refresh]);
  // A round starting or ending, or another device's data arriving.
  useEffect(() => {
    const off = onSyncStatus(refresh);
    return () => {
      void off.then((stop) => stop());
    };
  }, [refresh]);
  useSyncReload(refresh);
  // "2 min ago" moves on by itself.
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);

  const info = (
    <>
      <p>
        Your devices sync straight with each other: no server, no account. Courses, words, progress, songs, the
        tutor's conversations and your API keys all go over. The theme, reminders and playback stay as each device has
        them.
      </p>
      <p>
        Both devices need to be on the same Wi-Fi with Polski open. Changes go over within seconds. The phone also syncs
        in the background about every half hour when it is on Wi-Fi.
      </p>
      <p>
        A computer's firewall can stop the phone from reaching it. That's fine: the computer asks the phone every few
        seconds while Polski is open on it.
      </p>
    </>
  );

  if (!status) {
    return (
      <Screen title="Devices" onBack={onBack} info={info}>
        {error ? <Banner tone="error">{error}</Banner> : <Loading />}
      </Screen>
    );
  }

  const others = status.devices.filter((entry) => !entry.isSelf);
  const synced = lastSynced(status);

  return (
    <Screen title="Devices" onBack={onBack} info={info}>
      {error ? <Banner tone="error">{error}</Banner> : null}

      <Section title="This device">
        <List>
          <ListItem
            leading={<DevicesIcon />}
            title={status.name}
            subtitle={`${platformName(status.platform)} · tap to rename`}
            onClick={() => setSheet("rename")}
          />
        </List>
      </Section>

      {status.inVault ? (
        <>
          <Section title="Your other devices">
            <List>
              {others.length === 0 ? (
                <ListEmpty>None yet. Add one below.</ListEmpty>
              ) : (
                others.map((entry) => (
                  <ListItem
                    key={entry.id}
                    title={entry.name}
                    subtitle={deviceLine(entry, now)}
                    onClick={() => setDevice(entry)}
                  />
                ))
              )}
            </List>
          </Section>

          <Section title="Sync">
            <Stack gap={3}>
              <SyncLine status={status} synced={synced} now={now} />
              <Button
                variant="primary"
                wide
                loading={status.syncing}
                onClick={() => {
                  api.syncNow().then(
                    () => setStatus({ ...status, syncing: true }),
                    (cause) => toast(errorMessage(cause), "error"),
                  );
                }}
              >
                {status.syncing ? "Syncing…" : "Sync now"}
              </Button>
            </Stack>
          </Section>
        </>
      ) : (
        <Text tone="dim" as="p">
          Keep your phone and computer in step: the same courses, words and progress on both. Add your other device
          here, or join it from here if it already has your data.
        </Text>
      )}

      <Section title={status.inVault ? "More devices" : "Connect"}>
        <List>
          <ListItem
            leading={<QrIcon />}
            title="Add a device"
            subtitle="Show a code for your other device to scan"
            onClick={() => setSheet("offer")}
          />
          <ListItem
            leading={<ScanIcon />}
            title="Join another device"
            subtitle={canScan ? "Scan the code it shows, and take its data" : "Paste the code it shows, and take its data"}
            onClick={() => setSheet("join")}
          />
        </List>
      </Section>

      {sheet === "rename" ? (
        <RenameSheet
          name={status.name}
          onClose={() => setSheet(null)}
          onSaved={() => {
            setSheet(null);
            refresh();
          }}
        />
      ) : null}
      {sheet === "offer" ? (
        <OfferSheet
          status={status}
          onClose={() => {
            setSheet(null);
            refresh();
          }}
        />
      ) : null}
      {sheet === "join" ? (
        <JoinSheet
          onClose={() => {
            setSheet(null);
            refresh();
          }}
        />
      ) : null}
      {device ? (
        <DeviceSheet
          device={device}
          now={now}
          onClose={() => setDevice(null)}
          onRemoved={() => {
            setDevice(null);
            refresh();
          }}
        />
      ) : null}
    </Screen>
  );
}

/** Syncing, when it last did, or what went wrong, in plain words. */
function SyncLine({ status, synced, now }: { status: SyncStatus; synced: string | null; now: number }) {
  if (status.syncing) {
    return (
      <Inline gap={2}>
        <Spinner />
        <Text tone="dim">Syncing…</Text>
      </Inline>
    );
  }
  return (
    <Stack gap={1}>
      <Text tone="dim">{synced ? `Last synced ${ago(synced, now)}.` : "Not synced yet."}</Text>
      {status.lastError ? <Text tone="bad">{syncErrorText(status.lastError)}</Text> : null}
    </Stack>
  );
}

function RenameSheet({ name, onClose, onSaved }: { name: string; onClose: () => void; onSaved: () => void }) {
  const [draft, setDraft] = useState(name);
  const [busy, setBusy] = useState(false);
  const trimmed = draft.trim();
  const save = async () => {
    setBusy(true);
    try {
      await api.syncSetName(trimmed);
      onSaved();
    } catch (cause) {
      toast(errorMessage(cause), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <Sheet title="This device's name" onClose={onClose}>
      <Stack gap={3}>
        <Field label="Name" hint="How your other devices list this one">
          <input
            value={draft}
            maxLength={60}
            autoCapitalize="words"
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && trimmed.length > 0) void save();
            }}
          />
        </Field>
        <Button variant="primary" wide loading={busy} disabled={trimmed.length === 0} onClick={() => void save()}>
          Save
        </Button>
      </Stack>
    </Sheet>
  );
}

/** "9:41" left of a code. */
function timeLeft(expiresAt: string, now: number): string {
  const seconds = Math.max(0, Math.floor((Date.parse(expiresAt) - now) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/**
 * "Add a device": a one-time code, as a QR code and as text, good for ten
 * minutes. Closes by itself once the other device has joined.
 */
function OfferSheet({ status, onClose }: { status: SyncStatus; onClose: () => void }) {
  const [offer, setOffer] = useState<SyncOffer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  // The devices there were when the code was made: one more means it worked.
  const known = useRef(new Set(status.devices.map((entry) => entry.id)));
  const joined = useRef(false);

  const make = useCallback(() => {
    setError(null);
    setOffer(null);
    api.syncOffer().then(setOffer, (cause) => setError(syncErrorText(errorMessage(cause))));
  }, []);
  useEffect(make, [make]);

  // The countdown, and a look at the device list every couple of seconds
  // (as well as on every sync event) to notice the other device joining.
  useEffect(() => {
    const tick = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(tick);
  }, []);
  const check = useCallback(() => {
    api.syncStatus().then(
      (next) => {
        const added = next.devices.find((entry) => !entry.isSelf && !known.current.has(entry.id));
        if (added && !joined.current) {
          joined.current = true;
          toast(`${added.name} joined. They're in step now.`);
          onClose();
        }
      },
      () => undefined,
    );
  }, [onClose]);
  useEffect(() => {
    const poll = window.setInterval(check, 2000);
    const off = onSyncStatus(check);
    return () => {
      window.clearInterval(poll);
      void off.then((stop) => stop());
    };
  }, [check]);

  const cancel = () => {
    // The code stops working at once, not in ten minutes.
    if (!joined.current) void api.syncCancelOffer().catch(() => undefined);
    onClose();
  };
  const expired = offer !== null && Date.parse(offer.expiresAt) <= now;

  return (
    <Sheet title="Add a device" onClose={cancel}>
      <Stack gap={3}>
        {error ? (
          <Banner tone="error">{error}</Banner>
        ) : !offer ? (
          <Loading label="Making a code…" />
        ) : expired ? (
          <Stack gap={3}>
            <Text tone="dim" align="center">
              This code has expired.
            </Text>
            <Button variant="primary" wide onClick={make}>
              Show a new code
            </Button>
          </Stack>
        ) : (
          <>
            <div
              className="key-qr"
              role="img"
              aria-label="QR code for your other device"
              dangerouslySetInnerHTML={{ __html: offer.svg }}
            />
            <Text variant="small" tone="dim" align="center">
              On your other device, open Settings → Devices → Join another device, and scan this. Works once, for{" "}
              {timeLeft(offer.expiresAt, now)} more.
            </Text>
            <Text variant="caption" tone="faint" selectable className="sync-code">
              {offer.code}
            </Text>
            <Button
              size="sm"
              variant="plain"
              onClick={() =>
                void copyText(offer.code).then(
                  () => toast("Code copied. Paste it on the other device."),
                  (cause) => toast(errorMessage(cause), "error"),
                )
              }
            >
              Copy the code
            </Button>
          </>
        )}
        <Button wide onClick={cancel}>
          Cancel
        </Button>
      </Stack>
    </Sheet>
  );
}

type JoinStep =
  | { kind: "enter" }
  | { kind: "checking" }
  | { kind: "confirm"; code: string; held: string }
  | { kind: "joining" }
  | { kind: "done"; result: JoinResult }
  | { kind: "failed"; message: string };

/**
 * "Join another device": scan (or paste) the code the other device shows.
 * If this device holds data of its own, says what, and that joining replaces
 * it with the other device's (a backup is kept) before going ahead.
 */
function JoinSheet({ onClose }: { onClose: () => void }) {
  const [step, setStep] = useState<JoinStep>({ kind: "enter" });
  const [pasted, setPasted] = useState("");
  const [scanning, setScanning] = useState(false);

  const join = async (code: string, replace: boolean) => {
    setStep({ kind: "joining" });
    try {
      setStep({ kind: "done", result: await api.syncJoin(code, replace) });
    } catch (cause) {
      setStep({ kind: "failed", message: syncErrorText(errorMessage(cause)) });
    }
  };

  const begin = async (code: string) => {
    const trimmed = code.trim();
    if (!trimmed) return;
    setStep({ kind: "checking" });
    try {
      const held = await api.syncHeldData();
      if (held.length === 0) await join(trimmed, false);
      else setStep({ kind: "confirm", code: trimmed, held: describeHeld(held) });
    } catch (cause) {
      setStep({ kind: "failed", message: errorMessage(cause) });
    }
  };

  const scan = async () => {
    try {
      if (!(await cameraAllowed())) {
        toast("Polski needs the camera to scan. Allow it in the app's settings.");
        await openAppSettings().catch(() => undefined);
        return;
      }
      setScanning(true);
      const text = await scanQr();
      setScanning(false);
      if (text !== null) await begin(text);
    } catch (cause) {
      setScanning(false);
      toast(errorMessage(cause), "error");
    }
  };

  // The camera runs behind the page: the sheet steps aside meanwhile.
  if (scanning) return <ScanOverlay />;

  const busy = step.kind === "checking" || step.kind === "joining";
  return (
    <Sheet title="Join another device" onClose={busy ? () => undefined : onClose}>
      <Stack gap={3}>
        {step.kind === "enter" ? (
          <>
            <Text tone="dim">
              On the other device, open Settings → Devices → Add a device. Then{" "}
              {canScan ? "scan the code it shows." : "copy its code and paste it here."}
            </Text>
            {canScan ? (
              <Button variant="primary" wide icon={<ScanIcon />} onClick={() => void scan()}>
                Scan the code
              </Button>
            ) : null}
            <Field label={canScan ? "Or paste the code" : "The code"}>
              <textarea
                rows={3}
                value={pasted}
                placeholder="polski-pair:1:…"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
                onChange={(event) => setPasted(event.target.value)}
              />
            </Field>
            <Button
              variant={canScan ? "secondary" : "primary"}
              wide
              disabled={pasted.trim().length === 0}
              onClick={() => void begin(pasted)}
            >
              Join
            </Button>
          </>
        ) : null}

        {step.kind === "checking" ? <Loading label="Looking at what's on this device…" /> : null}

        {step.kind === "confirm" ? (
          <>
            <Text>
              This device has {step.held}. Joining replaces all of it with the other device's data.
            </Text>
            <Text tone="dim" variant="small">
              A backup copy of this device's data is kept first, in case you need it back.
            </Text>
            <Inline gap={2}>
              <Button onClick={onClose}>Cancel</Button>
              <div className="grow">
                <Button variant="danger" wide onClick={() => void join(step.code, true)}>
                  Replace
                </Button>
              </div>
            </Inline>
          </>
        ) : null}

        {step.kind === "joining" ? (
          <Loading label="Joining… copying the other device's data. Keep Polski open on both." />
        ) : null}

        {step.kind === "done" ? (
          <>
            <Banner tone="ok">Joined {step.result.outcome.peerName}. This device now has the same data.</Banner>
            {step.result.outcome.complete ? null : (
              <Text tone="dim" variant="small">
                A little is still to come; it follows on the next sync.
              </Text>
            )}
            {step.result.backup ? (
              <Text tone="dim" variant="small">
                What was here before is saved in a backup.
              </Text>
            ) : null}
            <Button variant="primary" wide onClick={onClose}>
              Done
            </Button>
          </>
        ) : null}

        {step.kind === "failed" ? (
          <>
            <Banner tone="error">{step.message}</Banner>
            <Inline gap={2}>
              <Button onClick={onClose}>Close</Button>
              <div className="grow">
                <Button variant="primary" wide onClick={() => setStep({ kind: "enter" })}>
                  Try again
                </Button>
              </div>
            </Inline>
          </>
        ) : null}
      </Stack>
    </Sheet>
  );
}

/** One of the other devices: when it last synced, and removing it. */
function DeviceSheet({
  device,
  now,
  onClose,
  onRemoved,
}: {
  device: SyncDevice;
  now: number;
  onClose: () => void;
  onRemoved: () => void;
}) {
  return (
    <Sheet title={device.name} onClose={onClose}>
      <Stack gap={4}>
        <Stack gap={1}>
          <Inline gap={2} wrap>
            <Badge>{platformName(device.platform)}</Badge>
            {device.lastSyncAt ? (
              <Badge tone={device.complete ? "mint" : "gold"}>{device.complete ? "Up to date" : "Catching up"}</Badge>
            ) : null}
          </Inline>
          <Text tone="dim">
            {device.lastSyncAt ? `Last synced ${ago(device.lastSyncAt, now)}.` : "Hasn't synced with this one yet."}
          </Text>
          {device.lastSyncAt && !device.complete ? (
            <Text tone="dim" variant="small">
              The last sync didn't end with both holding the same data; the next one carries on.
            </Text>
          ) : null}
        </Stack>
        <ConfirmButton
          label="Remove this device"
          confirmLabel="Remove"
          question={`Stop syncing with ${device.name}, for good? What's on it stays there. To sync it again, it has to join afresh.`}
          wide
          onConfirm={async () => {
            try {
              await api.syncRemoveDevice(device.id);
              toast(`${device.name} removed.`);
              onRemoved();
            } catch (cause) {
              toast(errorMessage(cause), "error");
            }
          }}
        />
      </Stack>
    </Sheet>
  );
}
