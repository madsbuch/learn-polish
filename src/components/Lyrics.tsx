import { sectionLabel } from "../lib/lyrics";
import type { SongSection } from "../lib/types";

/** A song's lyrics: English then Polish in the verses, Polish first in the chorus. */
export function Lyrics({ sections }: { sections: SongSection[] }) {
  return (
    <div className="lyrics selectable">
      {sections.map((section, i) => (
        <div key={i} className="lyric-section" data-kind={section.kind}>
          <div className="t-eyebrow">{sectionLabel(section.kind)}</div>
          {section.lines.map((line, j) =>
            section.kind === "chorus" ? (
              <div key={j}>
                <span className="lyric-pl">{line.pl}</span>
                {line.en ? <span className="lyric-en">{line.en}</span> : null}
              </div>
            ) : (
              <div key={j}>
                {line.en ? <span className="lyric-en">{line.en}</span> : null}
                <span className="lyric-pl">{line.pl}</span>
              </div>
            ),
          )}
        </div>
      ))}
    </div>
  );
}
