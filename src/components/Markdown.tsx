import { Fragment, type ReactNode } from "react";

/**
 * A small markdown renderer for AI answers.
 *
 * It builds React elements rather than HTML, so model output can never inject
 * markup — no `dangerouslySetInnerHTML`, no sanitiser to keep in step. It
 * covers what the prompts actually produce: headings, emphasis, inline and
 * fenced code, bullet and numbered lists (one level of nesting), block quotes,
 * horizontal rules and tables (lessons are full of conjugation tables).
 * Anything else falls through as plain text.
 */
export function Markdown({ text }: { text: string }) {
  return <div className="md selectable">{renderBlocks(text)}</div>;
}

type Block =
  | { kind: "heading"; level: number; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "code"; text: string; language: string }
  | { kind: "quote"; text: string }
  | { kind: "rule" }
  | { kind: "list"; ordered: boolean; items: ListItem[] }
  | { kind: "table"; head: string[]; rows: string[][] };

interface ListItem {
  text: string;
  children: ListItem[];
}

function renderBlocks(source: string): ReactNode[] {
  return parseBlocks(source).map((block, index) => {
    switch (block.kind) {
      case "heading": {
        // Markdown h1/h2 inside a card should not dwarf the screen's own
        // heading, so everything shifts down a couple of levels visually.
        const Tag = (`h${Math.min(6, block.level + 2)}`) as "h3";
        return (
          <Tag key={index} className={`md-h md-h${block.level}`}>
            {renderInline(block.text)}
          </Tag>
        );
      }
      case "paragraph":
        return <p key={index}>{renderInline(block.text)}</p>;
      case "code":
        return (
          <pre key={index} className="md-pre" data-language={block.language || undefined}>
            <code>{block.text}</code>
          </pre>
        );
      case "quote":
        return (
          <blockquote key={index} className="md-quote">
            {renderInline(block.text)}
          </blockquote>
        );
      case "rule":
        return <hr key={index} className="md-rule" />;
      case "list":
        return renderList(block, index);
      case "table":
        return (
          <div key={index} className="md-table-wrap">
            <table className="md-table">
              <thead>
                <tr>
                  {block.head.map((cell, i) => (
                    <th key={i}>{renderInline(cell)}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {block.rows.map((row, r) => (
                  <tr key={r}>
                    {block.head.map((_, c) => (
                      <td key={c}>{renderInline(row[c] ?? "")}</td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        );
    }
  });
}

function renderList(block: Extract<Block, { kind: "list" }>, key: number) {
  const Tag = block.ordered ? "ol" : "ul";
  return (
    <Tag key={key} className="md-list">
      {block.items.map((item, index) => (
        <li key={index}>
          {renderInline(item.text)}
          {item.children.length > 0 ? (
            <ul className="md-list">
              {item.children.map((child, childIndex) => (
                <li key={childIndex}>{renderInline(child.text)}</li>
              ))}
            </ul>
          ) : null}
        </li>
      ))}
    </Tag>
  );
}

const HEADING = /^(#{1,6})\s+(.*)$/;
const BULLET = /^(\s*)[-*+]\s+(.*)$/;
const NUMBERED = /^(\s*)\d+[.)]\s+(.*)$/;
const QUOTE = /^>\s?(.*)$/;
const RULE = /^(-{3,}|\*{3,}|_{3,})$/;
const FENCE = /^```\s*(\S*)\s*$/;
/** A table row: starts and ends with a pipe. */
const TABLE_ROW = /^\|.*\|$/;
/** The line under a table's header: | --- | :---: | */
const TABLE_RULE = /^\|(\s*:?-{3,}:?\s*\|)+$/;

function cells(line: string): string[] {
  return line
    .trim()
    .replace(/^\||\|$/g, "")
    .split("|")
    .map((cell) => cell.trim());
}
/** Two spaces of indent is enough to count as nested in model output. */
const NEST_WIDTH = 2;

function parseBlocks(source: string): Block[] {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const blocks: Block[] = [];
  let paragraph: string[] = [];

  const flush = () => {
    if (paragraph.length > 0) {
      blocks.push({ kind: "paragraph", text: paragraph.join(" ").trim() });
      paragraph = [];
    }
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    const fence = FENCE.exec(line.trim());
    if (fence) {
      flush();
      const body: string[] = [];
      i++;
      // An unterminated fence (still streaming) renders what has arrived.
      while (i < lines.length && !FENCE.test(lines[i].trim())) {
        body.push(lines[i]);
        i++;
      }
      blocks.push({ kind: "code", text: body.join("\n"), language: fence[1] });
      continue;
    }

    if (line.trim() === "") {
      flush();
      continue;
    }

    if (RULE.test(line.trim())) {
      flush();
      blocks.push({ kind: "rule" });
      continue;
    }

    if (TABLE_ROW.test(line.trim()) && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1].trim().replace(/\s/g, ""))) {
      flush();
      const head = cells(line);
      const rows: string[][] = [];
      i += 2;
      while (i < lines.length && TABLE_ROW.test(lines[i].trim())) {
        rows.push(cells(lines[i]));
        i++;
      }
      i--;
      blocks.push({ kind: "table", head, rows });
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      flush();
      blocks.push({ kind: "heading", level: heading[1].length, text: heading[2] });
      continue;
    }

    const quote = QUOTE.exec(line);
    if (quote) {
      flush();
      const body = [quote[1]];
      while (i + 1 < lines.length && QUOTE.test(lines[i + 1])) {
        body.push(QUOTE.exec(lines[++i])![1]);
      }
      blocks.push({ kind: "quote", text: body.join(" ") });
      continue;
    }

    if (BULLET.test(line) || NUMBERED.test(line)) {
      flush();
      const ordered = NUMBERED.test(line) && !BULLET.test(line);
      const items: ListItem[] = [];
      while (i < lines.length) {
        const match = BULLET.exec(lines[i]) ?? NUMBERED.exec(lines[i]);
        if (!match) break;
        const [, indent, text] = match;
        if (indent.length >= NEST_WIDTH && items.length > 0) {
          items[items.length - 1].children.push({ text, children: [] });
        } else {
          items.push({ text, children: [] });
        }
        i++;
      }
      i--;
      blocks.push({ kind: "list", ordered, items });
      continue;
    }

    paragraph.push(line.trim());
  }

  flush();
  return blocks;
}

/** Ordered so that longer markers are tried before their prefixes. */
const INLINE = [
  { pattern: /`([^`]+)`/, render: (body: string, key: number) => <code key={key} className="md-code">{body}</code> },
  { pattern: /\*\*([^*]+)\*\*/, render: (body: string, key: number) => <strong key={key}>{renderInline(body)}</strong> },
  { pattern: /__([^_]+)__/, render: (body: string, key: number) => <strong key={key}>{renderInline(body)}</strong> },
  { pattern: /\*([^*\n]+)\*/, render: (body: string, key: number) => <em key={key}>{renderInline(body)}</em> },
  { pattern: /_([^_\n]+)_/, render: (body: string, key: number) => <em key={key}>{renderInline(body)}</em> },
] as const;

function renderInline(text: string): ReactNode {
  // Find whichever marker appears first, render around it, then recurse.
  let best: { index: number; length: number; body: string; render: (body: string, key: number) => ReactNode } | null =
    null;

  for (const { pattern, render } of INLINE) {
    const match = pattern.exec(text);
    if (match && (best === null || match.index < best.index)) {
      best = { index: match.index, length: match[0].length, body: match[1], render };
    }
  }

  if (!best) return text;

  const before = text.slice(0, best.index);
  const after = text.slice(best.index + best.length);
  return (
    <Fragment>
      {before}
      {best.render(best.body, best.index)}
      {renderInline(after)}
    </Fragment>
  );
}
