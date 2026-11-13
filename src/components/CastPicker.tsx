import { useEffect, useState } from "react";

import { castTo, discoverSpeakers, type QueueItem } from "../lib/player";
import type { CastDevice } from "../lib/types";
import { Banner, Spinner } from "./ui";
import { errorMessage } from "../lib/api";

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
    <div className="sheet-backdrop fade-in" onClick={onClose}>
      <div className="sheet" onClick={(event) => event.stopPropagation()}>
        <div className="row between" style={{ marginBottom: 6 }}>
          <h2 style={{ margin: 0 }}>Play on a speaker</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </div>
        <p className="small">
          Sonos speakers on this Wi-Fi. The songs stream from this phone, so keep it on the
          same network.
        </p>

        {error ? <Banner kind="error">{error}</Banner> : null}

        <div className="card flush" style={{ marginBottom: 12 }}>
          {devices === null || (searching && devices.length === 0) ? (
            <div className="row" style={{ padding: 16 }}>
              <Spinner /> <span className="small muted">Looking for speakers…</span>
            </div>
          ) : devices.length === 0 ? (
            <p className="small" style={{ padding: 16, margin: 0 }}>
              No Sonos found. Check that the phone is on the same Wi-Fi as the speakers, or
              type a speaker's IP address below (Sonos app → Settings → System → About My
              System).
            </p>
          ) : (
            devices.map((device) => (
              <button
                key={device.id}
                className="list-row"
                disabled={connecting !== null}
                onClick={async () => {
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
              >
                <span className="speaker-glyph">🔊</span>
                <span className="grow">
                  <span className="pl">{device.name}</span>
                  <span className="tr">{device.baseUrl.replace("http://", "")}</span>
                </span>
                {connecting === device.id ? <Spinner /> : <span className="faint">›</span>}
              </button>
            ))
          )}
        </div>

        <div className="row" style={{ gap: 8 }}>
          <input
            className="grow"
            value={address}
            placeholder="Speaker IP, e.g. 192.168.1.20"
            inputMode="decimal"
            onChange={(event) => setAddress(event.target.value)}
          />
          <button
            className="btn ghost small"
            disabled={searching}
            onClick={() => void search(address.trim() || undefined)}
          >
            {searching ? <Spinner /> : address.trim() ? "Connect" : "Search again"}
          </button>
        </div>
      </div>
    </div>
  );
}
