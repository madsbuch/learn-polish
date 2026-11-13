import { sectionLabel } from "../lib/lyrics";
import type { SongSection } from "../lib/types";

/** A song's lyrics: English then Polish in the verses, Polish first in the chorus. */
export function Lyrics({ sections }: { sections: SongSection[] }) {
  return (
    <div className="lyrics selectable">
      {sections.map((section, i) => (
        <div key={i} className="lyric-section" data-kind={section.kind}>
          <div className="eyebrow">{sectionLabel(section.kind)}</div>
          {section.lines.map((line, j) => (
            <div key={j} className="lyric-line">
              {section.kind === "chorus" ? (
                <>
                  <span className="pl">{line.pl}</span>
                  {line.en ? <span className="en">{line.en}</span> : null}
                </>
              ) : (
                <>
                  {line.en ? <span className="en">{line.en}</span> : null}
                  <span className="pl">{line.pl}</span>
                </>
              )}
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
