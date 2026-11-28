// The radio on the player's side (src/lib/player.ts): bridges in the queue.
// Run with `bun test`.
import { describe, expect, test } from "bun:test";

import { itemId, keptInLibrary, type QueueItem, type TakeItem } from "../src/lib/player";
import { BridgeSchema, SongSchema, type TrackId } from "../src/lib/schemas";

const id = (prefix: string, n: number) => `${prefix}_0${String(n).padStart(25, "0")}`;

function take(n: number, plays = 0): TakeItem {
  const song = SongSchema.parse({
    id: id("sg", n),
    title: `Song ${n}`,
    chorusPl: "",
    chorusEn: "",
    style: "",
    vocalGender: "",
    sections: [],
    status: "ready",
    error: null,
    createdAt: 0,
    tracks: [
      {
        id: id("tk", n),
        songId: id("sg", n),
        ord: 0,
        audioFileId: id("fl", n),
        imageFileId: null,
        audioPath: "",
        imagePath: null,
        duration: 120,
        playCount: plays,
        lines: null,
      },
    ],
  });
  return { song, track: song.tracks[0] };
}

function bridge(n: number): QueueItem {
  const bridge = BridgeSchema.parse({
    id: id("brg", n),
    songId: null,
    role: "station",
    lines: [{ pl: "Słuchasz Polskiego Radia.", en: "You are listening to Polski Radio." }],
    format: "pairs",
    audioFileId: id("fl", 100 + n),
    audioPath: "",
    duration: 4,
    timedLines: [],
    playCount: 0,
  });
  return { bridge };
}

function library(...items: TakeItem[]): Map<TrackId, TakeItem> {
  return new Map(items.map((item) => [item.track.id, item]));
}

describe("the radio's queue", () => {
  // Spec: RADIO-6, RADIO-13
  test("a library refresh keeps the bridges and updates the takes", () => {
    const queue = [take(1), bridge(1), take(2), bridge(2), take(3)];
    const fresh = take(2, 7);
    const kept = keptInLibrary({ queue, index: 2 }, library(take(1), fresh, take(3)));
    expect(kept.queue.map(itemId)).toEqual(queue.map(itemId));
    expect(kept.index).toBe(2);
    expect(kept.queue[2]?.track?.playCount).toBe(7);
  });

  test("a deleted take leaves the queue; the bridges around it stay", () => {
    const queue = [take(1), bridge(1), take(2), take(3)];
    const kept = keptInLibrary({ queue, index: 1 }, library(take(1), take(3)));
    expect(kept.queue.map(itemId)).toEqual([take(1), bridge(1), take(3)].map(itemId));
    expect(kept.index).toBe(1);
  });

  test("deleting what plays leaves nothing playing", () => {
    const queue = [take(1), bridge(1), take(2)];
    const kept = keptInLibrary({ queue, index: 2 }, library(take(1)));
    expect(kept.queue.map(itemId)).toEqual([take(1), bridge(1)].map(itemId));
    expect(kept.index).toBe(-1);
  });
});
