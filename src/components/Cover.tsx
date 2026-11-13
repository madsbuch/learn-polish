import { convertFileSrc } from "@tauri-apps/api/core";

/** A take's cover art, or a note when Suno sent none. */
export function Cover({ path, size }: { path: string | null; size: number }) {
  return path ? (
    <img className="cover" src={convertFileSrc(path)} alt="" style={{ width: size, height: size }} />
  ) : (
    <span className="cover placeholder" style={{ width: size, height: size, fontSize: size * 0.4 }}>
      ♪
    </span>
  );
}
