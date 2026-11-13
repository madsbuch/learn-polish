import { useState, type ReactNode } from "react";

/**
 * A small ⓘ that opens an explanation in a sheet. Screens stay short; the
 * detail is one tap away for anyone who wants it.
 */
export function Info({ title, children }: { title: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        className="info-btn"
        aria-label={`About: ${title}`}
        onClick={(event) => {
          event.stopPropagation();
          event.preventDefault();
          setOpen(true);
        }}
      >
        i
      </button>
      {open ? (
        <div className="sheet-backdrop fade-in" onClick={() => setOpen(false)}>
          <div className="sheet info-sheet" role="dialog" aria-label={title} onClick={(e) => e.stopPropagation()}>
            <div className="row between" style={{ marginBottom: 8 }}>
              <h2 style={{ margin: 0 }}>{title}</h2>
              <button className="icon-btn" onClick={() => setOpen(false)} aria-label="Close">
                ✕
              </button>
            </div>
            <div className="info-body">{children}</div>
          </div>
        </div>
      ) : null}
    </>
  );
}

/** A heading with an ⓘ beside it. */
export function TitleWithInfo({
  as: Tag = "h3",
  title,
  info,
  infoTitle,
}: {
  as?: "h1" | "h2" | "h3";
  title: ReactNode;
  info: ReactNode;
  infoTitle?: string;
}) {
  return (
    <Tag className="with-info">
      {title}
      <Info title={infoTitle ?? (typeof title === "string" ? title : "About")}>{info}</Info>
    </Tag>
  );
}
