import type { CSSProperties } from "react";

import { fileUrl } from "../lib/files";
import type { FileId } from "../lib/types";

/** A take's cover art (a file id), or a note when Suno sent none. */
export function Cover({ fileId, size }: { fileId: FileId | null; size: number }) {
  const style: CSSProperties = { "--cover-size": `${size}px` };
  return fileId !== null ? (
    <img className="cover" src={fileUrl(fileId)} alt="" style={style} />
  ) : (
    <span className="cover placeholder" style={style} aria-hidden="true">
      ♪
    </span>
  );
}
