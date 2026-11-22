// What a tapped notification opens (src/lib/notifications.ts), run with `bun test`.
import { describe, expect, test } from "bun:test";

import { parse } from "../src/lib/navigation";
import { addressOfTap } from "../src/lib/notifications";

describe("a tapped notification", () => {
  // Spec: SHELL-20
  test("the app's own carry their page", () => {
    expect(addressOfTap({ address: "/tutor/cv_01" })).toBe("/tutor/cv_01");
    expect(addressOfTap({ address: "/music/playing" })).toBe("/music/playing");
  });

  // Spec: SHELL-20
  test("the notification plugin's are known by their id", () => {
    for (let id = 42; id <= 49; id++) expect(addressOfTap({ notification: id })).toBe("/");
    expect(addressOfTap({ notification: 41 })).toBe("/settings/practice");
    expect(parse("/settings/practice")?.trail).toEqual([{ kind: "settings", page: "practice" }]);
  });

  // Spec: SHELL-20
  test("anything else opens nothing", () => {
    expect(addressOfTap({ notification: 7 })).toBeNull();
    expect(addressOfTap({})).toBeNull();
    expect(addressOfTap(null)).toBeNull();
    expect(addressOfTap({ address: 3 })).toBeNull();
  });
});
