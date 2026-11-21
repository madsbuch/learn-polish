import { KIND_ICON, useIdTitle, useOpener } from "../lib/ids";

/**
 * A TypeID in text, shown as what it names: an icon and the title, which
 * opens it when the app can. A deleted thing says so; the id itself stays
 * selectable underneath for copying.
 */
export function IdChip({ id }: { id: string }) {
  const entry = useIdTitle(id);
  const open = useOpener(id);
  const prefix = id.split("_")[0];
  const label = entry === null ? "…" : (entry.title ?? `deleted ${entry.kind}`);
  const content = (
    <>
      <span aria-hidden="true">{KIND_ICON[prefix] ?? "🔗"}</span> {label}
    </>
  );
  return open && entry?.title ? (
    <button type="button" className="id-chip" title={id} onClick={open}>
      {content}
    </button>
  ) : (
    <span
      className="id-chip"
      title={id}
      data-missing={entry !== null && !entry.title ? true : undefined}
    >
      {content}
    </span>
  );
}
