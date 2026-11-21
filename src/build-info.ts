/**
 * Which build this is: set by CI (the commit and when it was built, see
 * .github/workflows/release.yml); a local build says so. Shown on the
 * loading screen and in Diagnostics, so it's plain which version runs.
 */
const commit = import.meta.env.VITE_BUILD_COMMIT;
const time = import.meta.env.VITE_BUILD_TIME;

export const buildCommit: string =
  typeof commit === "string" && commit ? commit.slice(0, 7) : "local";
export const buildTime: string | null =
  typeof time === "string" && time ? time : null;

/** "a1b2c3d · Nov 19, 08:15" (or "local build"). */
export function buildLabel(): string {
  if (buildCommit === "local") return "local build";
  const when = buildTime ? new Date(buildTime) : null;
  const date =
    when && !Number.isNaN(when.getTime())
      ? when.toLocaleString(undefined, {
          month: "short",
          day: "numeric",
          hour: "2-digit",
          minute: "2-digit",
        })
      : null;
  return date ? `${buildCommit} · ${date}` : buildCommit;
}
