import { convertFileSrc } from "@tauri-apps/api/core";
import type { CSSProperties } from "react";

/** A take's cover art, or a note when Suno sent none. */
export function Cover({ path, size }: { path: string | null; size: number }) {
  const style = { "--cover-size": `${size}px` } as CSSProperties;
  return path ? (
    <img className="cover" src={convertFileSrc(path)} alt="" style={style} />
  ) : (
    <span className="cover placeholder" style={style} aria-hidden="true">
      ♪
    </span>
  );
}
