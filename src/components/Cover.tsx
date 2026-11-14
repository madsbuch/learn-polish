import type { CSSProperties } from "react";

import { fileUrl } from "../lib/files";

/** A take's cover art (a file id), or a note when Suno sent none. */
export function Cover({ fileId, size }: { fileId: number | null; size: number }) {
  const style = { "--cover-size": `${size}px` } as CSSProperties;
  return fileId !== null ? (
    <img className="cover" src={fileUrl(fileId)} alt="" style={style} />
  ) : (
    <span className="cover placeholder" style={style} aria-hidden="true">
      ♪
    </span>
  );
}
