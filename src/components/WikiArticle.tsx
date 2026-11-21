import { Fragment, useMemo, useRef, type ReactNode } from "react";

import { Choice, ChoiceRow } from "../design";

/**
 * A Wikipedia article laid out as on Wikipedia: the lead, an infobox of key
 * facts, a table of contents, and collapsible sections with their lists,
 * tables, figures and quotes.
 *
 * Wikipedia's HTML is read with DOMParser (which runs nothing and loads
 * nothing) and turned into React elements from a short list of known tags —
 * like Markdown, it never reaches the page as HTML, so nothing in an article
 * can inject markup or script. Anything unknown is reduced to its text.
 * Pictures come only from Wikimedia's image server. Links are shown as text:
 * the reader stays on its article.
 */
export function WikiArticle({ html }: { html: string }) {
  const article = useMemo(() => parseArticle(html), [html]);
  const sectionRefs = useRef<(HTMLElement | null)[]>([]);

  return (
    <div className="wiki selectable">
      {article.infobox ? (
        <details className="wiki-infobox" open>
          <summary>At a glance</summary>
          <div className="wiki-table">
            <table>{renderChildren(article.infobox, "infobox")}</table>
          </div>
        </details>
      ) : null}

      {article.lead.map((node, i) => renderNode(node, `lead-${i}`))}

      {article.sections.length >= 3 ? (
        <nav className="wiki-contents" aria-label="Contents">
          <ChoiceRow>
            {article.sections.map((section, i) => (
              <Choice
                key={i}
                selected={false}
                onClick={() => {
                  const target = sectionRefs.current[i];
                  if (target instanceof HTMLDetailsElement) target.open = true;
                  target?.scrollIntoView({ behavior: "smooth", block: "start" });
                }}
              >
                {section.title}
              </Choice>
            ))}
          </ChoiceRow>
        </nav>
      ) : null}

      {article.sections.map((section, i) => (
        <details
          key={i}
          className="wiki-section"
          open
          ref={(element) => {
            sectionRefs.current[i] = element;
          }}
        >
          <summary className="wiki-section-title">{section.title}</summary>
          {section.nodes.map((node, j) => renderNode(node, `s${i}-${j}`))}
        </details>
      ))}
    </div>
  );
}

// ------------------------------------------------------------------ parsing

interface Section {
  title: string;
  nodes: Node[];
}

interface Article {
  lead: Node[];
  infobox: Element | null;
  sections: Section[];
}

/** Clutter that only makes sense on the website: edit links, footnote marks,
 * navigation boxes, maintenance notices. */
const REMOVE = [
  "script",
  "style",
  "link",
  "meta",
  "noscript",
  ".mw-editsection",
  "sup.reference",
  ".reference",
  ".mw-references-wrap",
  "ol.references",
  ".navbox",
  ".navbox-styles",
  ".vertical-navbox",
  ".sidebar",
  ".metadata",
  ".ambox",
  ".ombox",
  ".hatnote",
  ".dablink",
  ".noprint",
  ".mw-empty-elt",
  ".sistersitebox",
  ".portalbox",
  ".shortdescription",
  ".mw-jump-link",
  ".catlinks",
  "#toc",
  ".toc",
  ".printfooter",
  ".mw-cite-backlink",
  "#coordinates",
  ".geo-nondefault",
].join(",");

/** Sections of references and links elsewhere, in English, Danish and Polish. */
const SKIPPED_SECTIONS = new Set([
  "see also",
  "references",
  "notes",
  "citations",
  "footnotes",
  "external links",
  "further reading",
  "sources",
  "bibliography",
  "works cited",
  "literature",
  "se også",
  "referencer",
  "kilder",
  "eksterne henvisninger",
  "henvisninger",
  "litteratur",
  "noter",
  "fodnoter",
  "zobacz też",
  "przypisy",
  "linki zewnętrzne",
  "bibliografia",
  "uwagi",
]);

export function parseArticle(html: string): Article {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const root = doc.querySelector(".mw-parser-output") ?? doc.body;
  root.querySelectorAll(REMOVE).forEach((element) => element.remove());

  const infobox = root.querySelector("table.infobox");
  infobox?.remove();

  const lead: Node[] = [];
  const sections: Section[] = [];
  let current: Section | null = null;
  let skipping = false;

  for (const node of Array.from(root.childNodes)) {
    const heading = headingOf(node);
    if (heading && heading.level === 2) {
      skipping = SKIPPED_SECTIONS.has(heading.title.toLowerCase());
      current = skipping ? null : { title: heading.title, nodes: [] };
      if (current) sections.push(current);
      continue;
    }
    if (skipping) continue;
    if (node.nodeType === Node.TEXT_NODE && !node.textContent?.trim()) continue;
    (current ? current.nodes : lead).push(node);
  }
  return { lead, infobox, sections: sections.filter((s) => s.nodes.length > 0) };
}

/** A section heading: `<h2>`, or the newer `<div class="mw-heading"><h2>`. */
function headingOf(node: Node): { level: number; title: string } | null {
  if (!(node instanceof Element)) return null;
  const element = node.classList.contains("mw-heading") ? node.querySelector("h1,h2,h3,h4,h5,h6") : node;
  if (!element || !/^H[1-6]$/.test(element.tagName)) return null;
  return { level: Number(element.tagName[1]), title: (element.textContent ?? "").trim() };
}

// ---------------------------------------------------------------- rendering

/** Only Wikimedia's image server; protocol-relative addresses made https. */
function imageSource(src: string | null): string | null {
  if (!src) return null;
  const url = src.startsWith("//") ? `https:${src}` : src;
  return url.startsWith("https://upload.wikimedia.org/") ? url : null;
}

function span(value: string | null): number | undefined {
  const n = Number(value);
  return Number.isInteger(n) && n > 1 ? Math.min(n, 20) : undefined;
}

function renderChildren(node: Node, key: string): ReactNode[] {
  return Array.from(node.childNodes).map((child, i) => renderNode(child, `${key}.${i}`));
}

function renderNode(node: Node, key: string): ReactNode {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent;
  if (!(node instanceof Element)) return null;

  const heading = headingOf(node);
  if (heading) {
    return (
      <h4 key={key} className="wiki-subheading">
        {heading.title}
      </h4>
    );
  }

  const children = () => renderChildren(node, key);
  const tag = node.tagName.toLowerCase();

  // Math: its TeX, as text.
  if (node.classList.contains("mwe-math-element")) {
    const tex = node.querySelector("annotation")?.textContent ?? node.querySelector("img")?.getAttribute("alt") ?? "";
    return (
      <code key={key} className="wiki-math">
        {tex.trim()}
      </code>
    );
  }

  switch (tag) {
    case "p":
      return <p key={key}>{children()}</p>;
    case "b":
    case "strong":
      return <strong key={key}>{children()}</strong>;
    case "i":
    case "em":
    case "cite":
      return <em key={key}>{children()}</em>;
    case "sub":
      return <sub key={key}>{children()}</sub>;
    case "sup":
      return <sup key={key}>{children()}</sup>;
    case "small":
      return <small key={key}>{children()}</small>;
    case "br":
      return <br key={key} />;
    case "q":
      return <Fragment key={key}>“{children()}”</Fragment>;
    case "a":
      return (
        <span key={key} className="wiki-link">
          {children()}
        </span>
      );
    case "ul":
      return (
        <ul key={key} className="wiki-list">
          {children()}
        </ul>
      );
    case "ol":
      return (
        <ol key={key} className="wiki-list">
          {children()}
        </ol>
      );
    case "li":
      return <li key={key}>{children()}</li>;
    case "dl":
      return (
        <dl key={key} className="wiki-dl">
          {children()}
        </dl>
      );
    case "dt":
      return <dt key={key}>{children()}</dt>;
    case "dd":
      return <dd key={key}>{children()}</dd>;
    case "blockquote":
      return (
        <blockquote key={key} className="wiki-quote">
          {children()}
        </blockquote>
      );
    case "pre":
      return (
        <pre key={key} className="wiki-pre">
          {node.textContent}
        </pre>
      );
    case "code":
      return <code key={key}>{children()}</code>;
    case "table":
      return (
        <div key={key} className="wiki-table">
          <table>{children()}</table>
        </div>
      );
    case "caption":
      return <caption key={key}>{children()}</caption>;
    case "thead":
      return <thead key={key}>{children()}</thead>;
    case "tbody":
      return <tbody key={key}>{children()}</tbody>;
    case "tfoot":
      return <tfoot key={key}>{children()}</tfoot>;
    case "tr":
      return <tr key={key}>{children()}</tr>;
    case "th":
    case "td": {
      const Cell = tag;
      return (
        <Cell key={key} colSpan={span(node.getAttribute("colspan"))} rowSpan={span(node.getAttribute("rowspan"))}>
          {children()}
        </Cell>
      );
    }
    case "figure":
      return renderFigure(node, key);
    case "img": {
      const src = imageSource(node.getAttribute("src"));
      if (!src) return null;
      return <img key={key} className="wiki-image" src={src} alt={node.getAttribute("alt") ?? ""} loading="lazy" />;
    }
    case "figcaption":
      return <figcaption key={key}>{children()}</figcaption>;
    case "div":
      // Older markup for a picture with a caption.
      if (node.classList.contains("thumb")) return renderFigure(node, key);
      return <Fragment key={key}>{children()}</Fragment>;
    case "style":
    case "script":
    case "svg":
    case "math":
    case "input":
    case "button":
      return null;
    default:
      // span, section, abbr, bdi, time, mark, s, u, … : their content.
      return <Fragment key={key}>{children()}</Fragment>;
  }
}

/** A picture with its caption. */
function renderFigure(node: Element, key: string): ReactNode {
  const src = imageSource(node.querySelector("img")?.getAttribute("src") ?? null);
  const caption = node.querySelector("figcaption, .thumbcaption");
  if (!src) return caption ? <p key={key}>{renderChildren(caption, key)}</p> : null;
  return (
    <figure key={key} className="wiki-figure">
      <img src={src} alt={node.querySelector("img")?.getAttribute("alt") ?? ""} loading="lazy" />
      {caption ? <figcaption>{renderChildren(caption, key)}</figcaption> : null}
    </figure>
  );
}
