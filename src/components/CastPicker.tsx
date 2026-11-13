import { useEffect, useState } from "react";

import { Banner, Button, Inline, List, ListEmpty, ListItem, Sheet, Spinner, Stack, Text } from "../design";
import { errorMessage } from "../lib/api";
import { castTo, discoverSpeakers, type QueueItem } from "../lib/player";
import type { CastDevice } from "../lib/types";
import { SpeakerIcon } from "./icons";

/**
 * Lists the Sonos rooms on this Wi-Fi and moves the music to one. Searching
 * starts as soon as the sheet opens; an address can be typed for networks
 * that block discovery.
 */
export function CastPicker({
  fallbackQueue,
  onClose,
}: {
  fallbackQueue: QueueItem[];
  onClose: () => void;
}) {
  const [devices, setDevices] = useState<CastDevice[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [address, setAddress] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function search(manual?: string) {
    setSearching(true);
    setError(null);
    try {
      setDevices(await discoverSpeakers(manual));
    } catch (cause) {
      setError(errorMessage(cause));
      setDevices([]);
    } finally {
      setSearching(false);
    }
  }

  useEffect(() => {
    void search();
  }, []);

  return (
    <Sheet title="Play on a speaker" onClose={onClose}>
      {error ? <Banner tone="error">{error}</Banner> : null}
      <List>
        {devices === null || (searching && devices.length === 0) ? (
          <ListEmpty>
            <Inline gap={2}>
              <Spinner /> Looking for Sonos speakers on this Wi-Fi…
            </Inline>
          </ListEmpty>
        ) : devices.length === 0 ? (
          <ListEmpty>No speakers found. Type a speaker's IP address below.</ListEmpty>
        ) : (
          devices.map((device) => (
            <ListItem
              key={device.id}
              leading={<SpeakerIcon />}
              title={device.name}
              subtitle={device.baseUrl.replace("http://", "")}
              trailing={connecting === device.id ? <Spinner /> : undefined}
              chevron={connecting !== device.id}
              onClick={async () => {
                if (connecting) return;
                setConnecting(device.id);
                setError(null);
                try {
                  await castTo(device, fallbackQueue);
                  onClose();
                } catch (cause) {
                  setError(errorMessage(cause));
                } finally {
                  setConnecting(null);
                }
              }}
            />
          ))
        )}
      </List>
      <Stack gap={2}>
        <Inline gap={2}>
          <input
            value={address}
            placeholder="Speaker IP, e.g. 192.168.1.20"
            inputMode="decimal"
            onChange={(event) => setAddress(event.target.value)}
          />
          <Button loading={searching} onClick={() => void search(address.trim() || undefined)}>
            {address.trim() ? "Connect" : "Search"}
          </Button>
        </Inline>
        <Text variant="caption" tone="faint">
          The songs stream from this phone, so keep it on the same Wi-Fi. A speaker's IP is in
          the Sonos app under Settings → System → About My System.
        </Text>
      </Stack>
    </Sheet>
  );
}
