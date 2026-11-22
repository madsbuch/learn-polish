// The addresses and the history's shape (src/lib/navigation.ts): pure parts,
// run with `bun test`.
import { describe, expect, test } from "bun:test";

import {
  chain,
  format,
  forResume,
  HOME,
  MAX_DEPTH,
  openFrom,
  parse,
  parseLenient,
  planHistory,
  swapLast,
  up,
  type Address,
} from "../src/lib/navigation";

const id = (prefix: string, n: number) => `${prefix}_0${String(n).padStart(25, "0")}`;
const crs = id("crs", 1);
const ls = id("ls", 2);
const ls2 = id("ls", 3);
const cv = id("cv", 4);
const sg = id("sg", 5);
const wd = id("wd", 6);
const src = id("src", 7);

function at(path: string): Address {
  const address = parse(path);
  if (!address) throw new Error(`not an address: ${path}`);
  return address;
}

const paths = (address: Address) => chain(address).map(format);

describe("addresses", () => {
  // Spec: SHELL-16
  test("every page's address reads back as itself", () => {
    for (const path of [
      "/",
      `/${crs}`,
      `/${crs}/words`,
      `/${crs}/sentences/${ls}`,
      `/${crs}/${ls}/drill`,
      `/${crs}/${ls}/drill;challenge`,
      `/${crs}/${ls}/talk`,
      `/${crs}/${ls}/done`,
      `/${crs}/${ls}/${src}`,
      `/${ls}`,
      "/search",
      "/games",
      "/games/match",
      "/day/2026-11-22",
      "/progress",
      "/progress/day/2026-11-20",
      "/progress/achievements/streak-7",
      `/progress/words/${wd}/item/${encodeURIComponent(wd)}`,
      "/progress/mistakes/all",
      "/progress/hard/sound",
      "/progress/structure/case%3Ainstrumental",
      `/progress/topics/topic/food/log;f=${encodeURIComponent(JSON.stringify({ topic: "food" }))};t=Food`,
      `/progress/structures/new`,
      "/tutor",
      "/tutor/new",
      `/tutor/${cv}`,
      `/tutor/${cv}/${crs}/${ls}`,
      `/tutor/${cv}/settings/memories`,
      "/tutor/settings/ai",
      "/music",
      `/music/${sg}`,
      `/music/${sg}/${src}`,
      `/music/${sg}/${ls}/drill`,
      `/music/${id("pl", 8)}`,
      "/music/new/podcast",
      "/music/playing",
      "/settings",
      "/settings/how-to",
      "/settings/songs/ai",
      "/settings/sound/audio",
      `/settings/sound/audio/${crs}`,
    ]) {
      expect(format(at(path))).toBe(path);
    }
  });

  // Spec: SHELL-16
  test("ids are checked: a malformed one ends the address there", () => {
    expect(parse(`/crs_nope/${ls}`)).toBeNull();
    expect(parse("/tutor/cv_123")).toBeNull();
    const lenient = parseLenient(`/${crs}/ls_bad/drill`);
    expect(lenient.complete).toBe(false);
    expect(format(lenient.address)).toBe(`/${crs}`);
    // A drill belongs to a lesson; a playlist to the Music tab.
    expect(parse(`/${crs}/drill`)).toBeNull();
    expect(parse(`/tutor/${id("pl", 8)}`)).toBeNull();
  });
});

describe("back goes up", () => {
  // Spec: SHELL-3
  test("a page goes back to the page it was opened from, then the tab, then Exercises", () => {
    expect(paths(at(`/tutor/${cv}/${crs}/${ls}`))).toEqual([
      "/",
      "/tutor",
      `/tutor/${cv}`,
      `/tutor/${cv}/${crs}`,
      `/tutor/${cv}/${crs}/${ls}`,
    ]);
    expect(up(HOME)).toBeNull();
    expect(up(at("/music"))).toEqual(HOME);
  });

  // Spec: SHELL-3
  test("a summary goes back to where its lesson was started from", () => {
    expect(format(up(at(`/tutor/${cv}/${ls}/done`)) ?? HOME)).toBe(`/tutor/${cv}`);
    expect(format(up(at(`/music/${sg}/${ls}/done`)) ?? HOME)).toBe(`/music/${sg}`);
    expect(format(up(at(`/${ls}/done`)) ?? HOME)).toBe("/");
  });
});

describe("opening from a page", () => {
  // Spec: SHELL-17
  test("a chip opens above the page it is on, in its tab", () => {
    expect(format(openFrom(at(`/tutor/${cv}`), { kind: "lesson", id: lessonId(ls) }))).toBe(`/tutor/${cv}/${ls}`);
    expect(format(openFrom(at(`/progress/words/${wd}`), { kind: "lesson", id: lessonId(ls) }))).toBe(
      `/progress/words/${wd}/${ls}`,
    );
  });

  // Spec: SHELL-25
  test("a page already in the trail is gone back to, and the trail is capped", () => {
    const a = at(`/${crs}/${ls}/${ls2}`);
    expect(format(openFrom(a, { kind: "lesson", id: lessonId(ls) }))).toBe(`/${crs}/${ls}`);
    let deep = at("/tutor");
    for (let n = 0; n < MAX_DEPTH + 4; n++) deep = openFrom(deep, { kind: "lesson", id: lessonId(id("ls", 100 + n)) });
    expect(deep.trail.length).toBe(MAX_DEPTH);
    expect(deep.tab).toBe("tutor");
  });

  // Spec: SHELL-18
  test("a course's section is part of its address", () => {
    const course = at(`/${crs}`);
    const first = course.trail[0];
    if (first.kind !== "course") throw new Error("not a course");
    expect(format(swapLast(course, { ...first, section: "words" }))).toBe(`/${crs}/words`);
    expect(paths(at(`/${crs}/words/${ls}`))).toEqual(["/", `/${crs}/words`, `/${crs}/words/${ls}`]);
  });
});

describe("the history's shape", () => {
  // Spec: SHELL-3, SHELL-16
  test("into a page pushes; back to a page above goes back; a sibling replaces", () => {
    const h = (path: string) => paths(at(path));
    expect(planHistory(h(`/${crs}`), h(`/${crs}/${ls}`))).toEqual({
      back: 0,
      replace: null,
      push: [`/${crs}/${ls}`],
      isReturn: false,
    });
    expect(planHistory(h(`/${crs}/${ls}`), h(`/${crs}`))).toEqual({ back: 1, replace: null, push: [], isReturn: true });
    // Drill → summary: the drill's entry becomes the summary's, above the lesson's parent.
    expect(planHistory(h(`/${crs}/${ls}/drill`), h(`/${crs}/${ls}/done`))).toEqual({
      back: 1,
      replace: `/${crs}/${ls}/done`,
      push: [],
      isReturn: false,
    });
  });

  // Spec: SHELL-24
  test("picking another item beside a list replaces the one open", () => {
    expect(planHistory(paths(at("/settings/ai")), paths(at("/settings/sound")))).toEqual({
      back: 0,
      replace: "/settings/sound",
      push: [],
      isReturn: false,
    });
  });

  // Spec: SHELL-3
  test("another tab sits on Exercises, so back from it goes there", () => {
    expect(planHistory(paths(at(`/tutor/${cv}`)), paths(at("/music")))).toEqual({
      back: 1,
      replace: "/music",
      push: [],
      isReturn: false,
    });
  });
});

describe("a summary's misses", () => {
  // Spec: SUM-7, SHELL-3
  test("a missed word opens above the summary, and back returns to it", () => {
    const summary = at(`/${crs}/${ls}/done`);
    const item = openFrom(summary, { kind: "insight", page: { page: "item", itemId: wd } });
    expect(format(item)).toBe(`/${crs}/${ls}/done/item/${wd}`);
    expect(parse(format(item))).toEqual(item);
    expect(format(up(item) ?? HOME)).toBe(`/${crs}/${ls}/done`);
    expect(format(forResume(item))).toBe(`/${crs}/${ls}`);
  });
});

describe("reopening", () => {
  // Spec: SHELL-19
  test("a drill, a summary, a conversation, the player and a new tutor conversation are not reopened", () => {
    expect(format(forResume(at(`/${crs}/${ls}/drill`)))).toBe(`/${crs}/${ls}`);
    expect(format(forResume(at(`/${crs}/${ls}/done`)))).toBe(`/${crs}/${ls}`);
    expect(format(forResume(at(`/${ls}/talk`)))).toBe(`/${ls}`);
    expect(format(forResume(at("/music/playing")))).toBe("/music");
    expect(format(forResume(at("/tutor/new")))).toBe("/tutor");
    expect(format(forResume(at(`/tutor/${cv}`)))).toBe(`/tutor/${cv}`);
  });
});

function lessonId(text: string) {
  const address = at(`/${text}`);
  const first = address.trail[0];
  if (first.kind !== "lesson") throw new Error("not a lesson");
  return first.id;
}
