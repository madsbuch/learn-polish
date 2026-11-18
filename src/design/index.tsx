/**
 * The design system. Screens are built from these components and nothing
 * else: they never set a font size, a colour or a spacing by hand. The rules
 * are in DESIGN.md; `scripts/check-design.mjs` enforces the mechanical ones.
 */
import { useCallback, useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { createPortal } from "react-dom";

import { BackIcon, ChevronIcon, CloseIcon, DownIcon } from "../components/icons";
import { useBackHandler } from "../lib/back";
import { current as currentToasts, dismiss as dismissToast, subscribe as subscribeToasts } from "../lib/toast";
import {
  backward,
  prefersReducedMotion,
  recall,
  remember,
  ScreenEnterProvider,
  useEnterDirection,
  useScreenEnter,
  useStagger,
} from "./motion";
import "./tokens.css";
import "./base.css";
import "./components.css";

export {
  backward,
  CountUp,
  Confetti,
  markBack,
  prefersReducedMotion,
  recall,
  remember,
  useBump,
  useCountUp,
  useScreenEnter,
  useStagger,
  type ScreenEnter,
} from "./motion";

type Gap = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8;

function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(" ");
}

// ================================================================= layout

/** Children in a column, `gap` steps of the spacing scale apart. */
export function Stack({
  gap = 3,
  align,
  className,
  children,
}: {
  gap?: Gap;
  align?: "center";
  className?: string;
  children: ReactNode;
}) {
  return <div className={cx("stack", `gap-${gap}`, align, className)}>{children}</div>;
}

/** Children in a row, vertically centred. */
export function Inline({
  gap = 2,
  justify,
  align,
  wrap,
  className,
  children,
}: {
  gap?: Gap;
  justify?: "between" | "center";
  align?: "top";
  wrap?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return <div className={cx("inline", `gap-${gap}`, justify, align, wrap && "wrap", className)}>{children}</div>;
}

/** Takes the remaining room in an Inline. */
export function Grow({ children }: { children?: ReactNode }) {
  return <div className="grow">{children}</div>;
}

// ================================================================= text

type TextVariant = "display" | "headline" | "title" | "lead" | "body" | "small" | "caption" | "eyebrow";

/** Text on the type scale. `tone` dims it; `polish` marks Polish to learn. */
export function Text({
  variant = "body",
  tone,
  weight,
  align,
  ellipsis,
  clamp,
  selectable,
  as: Tag = "span",
  className,
  children,
}: {
  variant?: TextVariant;
  tone?: "dim" | "faint" | "brand" | "ok" | "bad" | "polish";
  weight?: "regular" | "medium" | "bold";
  align?: "center";
  ellipsis?: boolean;
  clamp?: 2;
  selectable?: boolean;
  as?: "span" | "p" | "div" | "h1" | "h2" | "h3";
  className?: string;
  children: ReactNode;
}) {
  return (
    <Tag
      className={cx(
        `t-${variant}`,
        tone && `tone-${tone}`,
        weight && `t-${weight}`,
        align && "t-center",
        ellipsis && "t-ellipsis",
        clamp && "t-clamp-2",
        selectable && "selectable",
        className,
      )}
    >
      {children}
    </Tag>
  );
}

// ================================================================= screens

/**
 * One screen: a scrolling column with a header. A tab's root screen has a
 * large title; a pushed screen (`onBack` or `onClose`) has a back or close
 * button and a smaller title. The body spaces its children by --space-6.
 */
export function Screen({
  title,
  subtitle,
  info,
  onBack,
  onClose,
  actions,
  header,
  nav = true,
  footer,
  backdrop,
  width = "reading",
  stagger,
  children,
}: {
  /** The body's blocks rise in one after another as it opens (a summary). */
  stagger?: boolean;
  /**
   * A picture behind the top of the page (an image URL), fading into the
   * background; it drifts and fades as the page scrolls. Not with `footer`.
   */
  backdrop?: string | null;
  /** Wide dashboards use the tablet canvas; lessons and chat stay readable. */
  width?: "reading" | "wide";
  title?: ReactNode;
  subtitle?: ReactNode;
  /** An explanation behind an ⓘ next to the title. */
  info?: ReactNode;
  onBack?: () => void;
  onClose?: () => void;
  /** Buttons at the right end of the header. Keep to one or two. */
  actions?: ReactNode;
  /** Replaces the standard header entirely (the home screen's greeting). */
  header?: ReactNode;
  /** False on screens that cover the tab bar. */
  nav?: boolean;
  /**
   * Docked below the scrolling content and above the tab bar,
   * e.g. the tutor's message box. It never scrolls away or floats.
   */
  footer?: ReactNode;
  children: ReactNode;
}) {
  const pushed = Boolean(onBack || onClose);
  // Pushed screens slide in from the right, slid-up ones rise, a tab's root
  // fades in; whatever is returned to comes back from the left.
  const enter = useEnterDirection(onClose && !onBack ? "up" : pushed ? "push" : "tab");
  const back = onBack ? backward(onBack) : undefined;
  const close = onClose ? backward(onClose) : undefined;
  // (Only a screen that asks for it starts the stagger's clock.)
  const bodyStagger = useStagger(Boolean(stagger));
  const bodyClass = cx("screen-body", stagger && bodyStagger);
  // The system back button does what the screen's own ← or ⌄ does.
  useBackHandler(pushed, back ?? close);
  const head = header ?? (
    <header className="screen-header" data-pushed={pushed}>
      {back ? <IconButton icon={<BackIcon />} label="Back" onClick={back} /> : null}
      {close ? <IconButton icon={<DownIcon />} label="Close" onClick={close} /> : null}
      <div className="screen-header__titles">
        {title ? (
          <h1 className="screen-header__title with-info">
            {title}
            {info ? <Info title={typeof title === "string" ? title : "About"}>{info}</Info> : null}
          </h1>
        ) : null}
        {subtitle ? (
          <Text variant="small" tone="dim" ellipsis>
            {subtitle}
          </Text>
        ) : null}
      </div>
      {actions ? <div className="screen-header__actions">{actions}</div> : null}
    </header>
  );
  if (!footer) {
    return (
      <ScreenEnterProvider value={enter}>
        <div className="screen" data-width={width} data-nav={nav} data-backdrop={Boolean(backdrop)} data-enter={enter}>
          {backdrop ? <Backdrop src={backdrop} /> : null}
          {head}
          <div className={bodyClass}>{children}</div>
        </div>
      </ScreenEnterProvider>
    );
  }
  // With a footer (a chat), the header is docked too: only the middle scrolls,
  // so Back and the input are always in reach.
  return (
    <ScreenEnterProvider value={enter}>
      <div className="screen-top" data-width={width} data-enter={enter}>
        {head}
      </div>
      <div className="screen" data-width={width} data-nav={nav} data-footer="true" data-enter={enter}>
        <div className={bodyClass}>{children}</div>
      </div>
      <div className="screen-footer" data-width={width} data-nav={nav} data-enter={enter}>
        {footer}
      </div>
    </ScreenEnterProvider>
  );
}

/** The window width from which list screens show what is open beside them. */
const SPLIT_MIN_WIDTH = 1000;

/** Whether a CSS media query matches now, following it as the window changes. */
export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => window.matchMedia?.(query).matches ?? false);
  useEffect(() => {
    const media = window.matchMedia?.(query);
    if (!media) return;
    const changed = () => setMatches(media.matches);
    changed();
    media.addEventListener("change", changed);
    return () => media.removeEventListener("change", changed);
  }, [query]);
  return matches;
}

/** True on a window wide enough for two panes (a tablet in landscape). */
export function useWideLayout(): boolean {
  return useMediaQuery(`(min-width: ${SPLIT_MIN_WIDTH}px)`);
}

/**
 * True on a tablet (or a wide window) held sideways: room to put two columns
 * beside each other on a screen of its own, such as the player.
 */
export function useLandscapeLayout(): boolean {
  return useMediaQuery("(min-width: 840px) and (orientation: landscape)");
}

/**
 * Two screens side by side on a wide window: a list, and what is open in it.
 * Each pane is a whole <Screen> with its own header, scrolling and footer.
 * Use it only when useWideLayout() is true; narrower windows show one
 * screen at a time.
 */
export function SplitView({ list, detail }: { list: ReactNode; detail: ReactNode }) {
  return (
    <div className="split">
      <div className="split-list">{list}</div>
      <div className="split-detail">{detail}</div>
    </div>
  );
}

/**
 * The picture behind a screen's top. It follows the scroll at a fraction of
 * the speed and fades out, so the page seems to slide over it.
 */
function Backdrop({ src }: { src: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const layer = ref.current;
    const scroller = layer?.parentElement;
    if (!layer || !scroller) return;
    let frame = 0;
    const update = () => {
      frame = 0;
      const y = scroller.scrollTop;
      // Pulled down past the top: it grows a little instead of leaving a gap.
      const stretch = y < 0 ? 1 + -y / 400 : 1;
      layer.style.transform = `translate3d(0, ${Math.max(0, y) * 0.55}px, 0) scale(${stretch})`;
      layer.style.opacity = String(Math.max(0, 1 - Math.max(0, y) / 320));
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    update();
    scroller.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      scroller.removeEventListener("scroll", onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, []);
  return (
    <div ref={ref} className="screen-backdrop" aria-hidden="true">
      <img src={src} alt="" />
    </div>
  );
}

/** A labelled group on a screen. The label is small and uppercase. */
export function Section({
  title,
  info,
  action,
  id,
  children,
}: {
  title?: ReactNode;
  info?: ReactNode;
  /** A single plain button at the right of the label, e.g. "Select". */
  action?: ReactNode;
  /** For scrolling to it. */
  id?: string;
  children: ReactNode;
}) {
  return (
    <section className="section" id={id}>
      {title || action ? (
        <div className="section-header">
          <h2 className="t-eyebrow with-info">
            {title}
            {info ? <Info title={typeof title === "string" ? title : "About"}>{info}</Info> : null}
          </h2>
          {action}
        </div>
      ) : null}
      {children}
    </section>
  );
}

// ================================================================= surfaces

export function Card({
  flush,
  onClick,
  className,
  children,
}: {
  flush?: boolean;
  onClick?: () => void;
  className?: string;
  children: ReactNode;
}) {
  if (onClick) {
    return (
      <button className={cx("card", className)} data-interactive="true" data-flush={flush} onClick={onClick}>
        {children}
      </button>
    );
  }
  return (
    <div className={cx("card", className)} data-flush={flush}>
      {children}
    </div>
  );
}

/** The brand gradient card. At most one per screen, at the top. */
export function Hero({ className, children }: { className?: string; children: ReactNode }) {
  return <div className={cx("hero", className)}>{children}</div>;
}

// ================================================================= lists

export function List({ stagger, children }: { stagger?: boolean; children: ReactNode }) {
  // `stagger`: its rows rise in one after another as the screen first shows them.
  const rise = useStagger(Boolean(stagger));
  return <div className={cx("list", stagger && rise)}>{children}</div>;
}

/**
 * A row in a List. With `onClick` the whole row (except `trailing`) is one
 * button, and a chevron shows it leads somewhere unless `chevron` is false.
 * Trailing controls are separate buttons, never nested inside the row's.
 */
export function ListItem({
  leading,
  title,
  subtitle,
  trailing,
  onClick,
  chevron,
  selected,
  label,
  selectable,
  oneLine,
  backdrop,
}: {
  /** A picture (image URL) behind the row, fading in from the left. */
  backdrop?: string | null;
  leading?: ReactNode;
  title: ReactNode;
  subtitle?: ReactNode;
  trailing?: ReactNode;
  onClick?: () => void;
  /**
   * Defaults to true for rows that open something, false otherwise. Set it
   * explicitly to show it next to a `trailing` label (which then opens the
   * row too); leave it out when `trailing` is a control of its own.
   */
  chevron?: boolean;
  selected?: boolean;
  /** Accessible name for the row button, when the title alone is unclear. */
  label?: string;
  selectable?: boolean;
  /** Keep the title to one line, cut off with an ellipsis. */
  oneLine?: boolean;
}) {
  const showChevron = chevron ?? (Boolean(onClick) && !trailing);
  // With an explicit chevron, `trailing` is a label (a badge, a duration),
  // not a control: it goes inside the row's button, before the chevron, so
  // the whole row opens it. Otherwise it is a control of its own (a 🔊).
  const trailingInside = Boolean(onClick) && chevron === true;
  const content = (
    <>
      {leading ? <span className="list-item__leading">{leading}</span> : null}
      <span className="list-item__text">
        <span className={cx("list-item__title", selectable && "selectable")} data-one-line={oneLine}>
          {title}
        </span>
        {subtitle ? <span className={cx("list-item__subtitle", selectable && "selectable")}>{subtitle}</span> : null}
      </span>
      {trailing && trailingInside ? <span className="list-item__trailing">{trailing}</span> : null}
      {showChevron ? (
        <span className="list-item__chevron" aria-hidden="true">
          <ChevronIcon />
        </span>
      ) : null}
    </>
  );
  // The row's button covers the whole row, chevron and edges included, so
  // a tap anywhere but on a trailing control opens it.
  return (
    <div
      className="list-item"
      data-selected={selected}
      data-interactive={Boolean(onClick)}
      data-backdrop={Boolean(backdrop)}
    >
      {backdrop ? (
        <span className="list-item__backdrop" aria-hidden="true">
          <img src={backdrop} alt="" loading="lazy" />
        </span>
      ) : null}
      {onClick ? (
        <button className="list-item__main" onClick={onClick} aria-label={label}>
          {content}
        </button>
      ) : (
        <div className="list-item__main">{content}</div>
      )}
      {trailing && !trailingInside ? <div className="list-item__trailing">{trailing}</div> : null}
    </div>
  );
}

export function ListEmpty({ children }: { children: ReactNode }) {
  return <div className="list-empty">{children}</div>;
}

/** A round mark at the start of a row: a number, initials or a symbol. */
export function Avatar({ children, muted }: { children: ReactNode; muted?: boolean }) {
  return (
    <span className="avatar" data-muted={muted}>
      {children}
    </span>
  );
}

// ================================================================= buttons

export type ButtonVariant = "primary" | "secondary" | "plain" | "danger" | "inverse" | "success";

/**
 * A text button. One primary button per screen or card; secondary for the
 * other choices; plain for low-emphasis actions in headers and rows; danger
 * only for things that delete.
 */
export function Button({
  variant = "secondary",
  size = "md",
  wide,
  icon,
  loading,
  disabled,
  onClick,
  label,
  children,
}: {
  variant?: ButtonVariant;
  size?: "sm" | "md" | "lg";
  wide?: boolean;
  icon?: ReactNode;
  loading?: boolean;
  disabled?: boolean;
  onClick?: () => void;
  label?: string;
  children: ReactNode;
}) {
  return (
    <button
      className={cx("button", wide ? null : "no-shrink")}
      data-variant={variant}
      data-size={size}
      data-wide={wide}
      disabled={disabled || loading}
      onClick={onClick}
      aria-label={label}
      aria-busy={loading || undefined}
    >
      {loading ? <Spinner /> : icon}
      {children}
    </button>
  );
}

/** A button that is only an icon. `label` is required: it is what a screen reader says. */
export function IconButton({
  icon,
  label,
  variant,
  size = "md",
  disabled,
  loading,
  failed,
  off,
  keepFocus,
  pulse,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  /** A soft halo breathing around it while something it started goes on (play, while playing). */
  pulse?: boolean;
  /**
   * Tapping it leaves focus (and the keyboard) where it is — for a send
   * button next to a text box. Otherwise the box loses focus first, the
   * keyboard drops, the layout jumps and the tap misses.
   */
  keepFocus?: boolean;
  /** A toggle that is currently off (shown dimmed, still tappable). */
  off?: boolean;
  variant?: "tinted" | "primary" | "inverse" | "inverse-solid";
  size?: "sm" | "md" | "lg" | "xl";
  disabled?: boolean;
  loading?: boolean;
  failed?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      className="icon-button"
      data-variant={variant}
      data-size={size}
      data-failed={failed || undefined}
      data-off={off || undefined}
      data-pulse={pulse || undefined}
      disabled={disabled || loading}
      onPointerDown={keepFocus ? (event) => event.preventDefault() : undefined}
      onMouseDown={keepFocus ? (event) => event.preventDefault() : undefined}
      onClick={(event) => {
        event.stopPropagation();
        onClick?.();
      }}
      aria-label={label}
      title={label}
    >
      {loading ? <Spinner /> : icon}
    </button>
  );
}

/** Icon-over-label actions sharing a row equally. */
export function ActionBar({ children }: { children: ReactNode }) {
  return <div className="action-bar">{children}</div>;
}

export function Action({
  icon,
  label,
  loading,
  disabled,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  loading?: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button onClick={onClick} disabled={disabled || loading}>
      {loading ? <Spinner /> : icon}
      <span>{label}</span>
    </button>
  );
}

// ================================================================= controls

/** An on/off setting. Use it, not a button labelled "On" or "Off". */
export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      className="switch"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    />
  );
}

/**
 * A setting in a List: label and description on the left, its control on the
 * right. `stacked` puts a wide control (a slider, a select) underneath.
 */
export function SettingRow({
  label,
  description,
  value,
  control,
  stacked,
}: {
  label: ReactNode;
  description?: ReactNode;
  /** The current value, shown next to the label for sliders. */
  value?: ReactNode;
  control?: ReactNode;
  stacked?: boolean;
}) {
  return (
    <div className="setting-row" data-stacked={stacked}>
      <div className="grow">
        <Inline justify="between">
          <span className="setting-row__label">{label}</span>
          {value !== undefined ? <span className="setting-row__value">{value}</span> : null}
        </Inline>
        {description ? <span className="setting-row__description">{description}</span> : null}
      </div>
      {control}
    </div>
  );
}

/** A labelled form control. */
export function Field({ label, hint, children }: { label: ReactNode; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="field">
      <span className="field__label">{label}</span>
      {children}
      {hint ? <span className="field__hint">{hint}</span> : null}
    </label>
  );
}

/** Two to four mutually exclusive views or values. */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: ReactNode }[];
  onChange: (value: T) => void;
}) {
  return (
    <div className="segmented" role="group">
      {options.map((option) => (
        <button key={option.value} aria-pressed={option.value === value} onClick={() => onChange(option.value)}>
          {option.label}
        </button>
      ))}
    </div>
  );
}

/** A horizontally scrolling row of filter choices. */
export function ChoiceRow({ children }: { children: ReactNode }) {
  return <div className="choice-row">{children}</div>;
}

export function Choice({
  selected,
  onClick,
  children,
}: {
  selected: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button className="choice" aria-pressed={selected} onClick={onClick}>
      {children}
    </button>
  );
}

// ================================================================= feedback

export type BadgeTone = "neutral" | "brand" | "gold" | "mint" | "sky" | "crimson";

/** A small status label. With `onClick` it is a button. */
export function Badge({
  tone = "neutral",
  onClick,
  label,
  children,
}: {
  tone?: BadgeTone;
  onClick?: () => void;
  label?: string;
  children: ReactNode;
}) {
  return onClick ? (
    <button className="badge" data-tone={tone} onClick={onClick} aria-label={label}>
      {children}
    </button>
  ) : (
    <span className="badge" data-tone={tone}>
      {children}
    </span>
  );
}

export function Banner({ tone = "info", children }: { tone?: "info" | "error" | "ok"; children: ReactNode }) {
  return (
    <div className="banner" data-tone={tone} role={tone === "error" ? "alert" : "status"}>
      {children}
    </div>
  );
}

export function Spinner() {
  return <span className="spinner" aria-label="Loading" />;
}

/** The other side of a chat is writing: three dots taking turns to light up. */
export function Typing() {
  return (
    <span className="typing" role="status" aria-label="Writing">
      <i />
      <i />
      <i />
    </span>
  );
}

export function Loading({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="center-screen">
      <Stack gap={3} align="center">
        <Spinner />
        <Text variant="small" tone="faint">
          {label}
        </Text>
      </Stack>
    </div>
  );
}

/**
 * A bar. It fills in as it appears (from where it stood when last seen, with
 * `memory`), and slides to a new value when it changes.
 */
export function Progress({
  value,
  max,
  tone,
  size,
  memory,
}: {
  value: number;
  max: number;
  tone?: "gold" | "mint";
  size?: "sm";
  /** A name to remember it by, e.g. "course:12": shown again, it moves on from there. */
  memory?: string;
}) {
  const pct = max > 0 ? Math.min(100, (value / max) * 100) : 0;
  const enter = useScreenEnter();
  // Where the fill starts as it appears: where it was last seen, or empty
  // on a new screen (a screen returned to shows it where it is).
  const [from] = useState(() => recall(memory) ?? (enter === "back" || prefersReducedMotion() ? pct : 0));
  useEffect(() => remember(memory, pct), [memory, pct]);
  return (
    <div
      className="progress"
      data-tone={tone}
      data-size={size}
      role="progressbar"
      aria-valuenow={value}
      aria-valuemax={max}
    >
      <span style={{ "--pct": pct, "--from": from }} />
    </div>
  );
}

export function StatGrid({ children }: { children: ReactNode }) {
  return <div className="stat-grid">{children}</div>;
}

export function Stat({ value, label }: { value: ReactNode; label: string }) {
  return (
    <div className="stat">
      <b>{value}</b>
      <span>{label}</span>
    </div>
  );
}

/** What a list shows when it has nothing in it: what it is, and how to fill it. */
export function EmptyState({
  icon,
  title,
  children,
  action,
}: {
  icon?: ReactNode;
  title: ReactNode;
  children?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      {icon ? <div className="empty-state__icon">{icon}</div> : null}
      <Text variant="lead" as="h3">
        {title}
      </Text>
      {children ? (
        <Text variant="small" tone="dim" as="p">
          {children}
        </Text>
      ) : null}
      {action}
    </div>
  );
}

// ================================================================= sheets

/** How long a sheet takes to slide away (components.css → sheet-out). */
const SHEET_CLOSE_MS = 180;

/** A panel that slides up from the bottom. Tapping outside closes it. */
export function Sheet({
  title,
  onClose,
  footer,
  children,
}: {
  title: ReactNode;
  onClose: () => void;
  /**
   * Stays at the bottom while the rest scrolls — a chat's message box, so
   * it sits on the keyboard instead of scrolling under it.
   */
  footer?: ReactNode;
  children: ReactNode;
}) {
  // Closing from the sheet itself (✕, outside, Escape, back) lets it slide
  // away first; closed from outside (a choice made in it), it just goes.
  const [closing, setClosing] = useState(false);
  const closeTimer = useRef(0);
  const dismiss = useCallback(() => {
    if (prefersReducedMotion()) {
      onClose();
      return;
    }
    if (closeTimer.current) return;
    setClosing(true);
    closeTimer.current = window.setTimeout(onClose, SHEET_CLOSE_MS);
  }, [onClose]);
  useEffect(() => () => window.clearTimeout(closeTimer.current), []);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => event.key === "Escape" && dismiss();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dismiss]);
  // Back closes the sheet, not the page under it.
  useBackHandler(true, dismiss);
  // Rendered at the top of the page, so it never takes on the look of where
  // it was opened from (an ⓘ inside a bold, single-line title, say).
  return createPortal(
    <div
      className="sheet-backdrop"
      data-closing={closing || undefined}
      onClick={(event) => {
        // A portal still passes clicks up to where it was opened from.
        event.stopPropagation();
        dismiss();
      }}
    >
      <div
        className="sheet"
        data-footer={footer ? true : undefined}
        role="dialog"
        aria-label={typeof title === "string" ? title : undefined}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="sheet-header">
          <h2>{title}</h2>
          <IconButton icon={<CloseIcon />} label="Close" onClick={dismiss} />
        </div>
        {footer ? (
          <>
            <div className="sheet-body">{children}</div>
            <div className="sheet-footer">{footer}</div>
          </>
        ) : (
          children
        )}
      </div>
    </div>,
    document.body,
  );
}

/** A small ⓘ that opens an explanation. Keeps screens short. */
export function Info({ title, children }: { title: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        className="info-button"
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
        <Sheet title={title} onClose={() => setOpen(false)}>
          <div className="info-body t-body">{children}</div>
        </Sheet>
      ) : null}
    </>
  );
}

/**
 * A destructive action that asks once. The first tap shows what will happen
 * and a second, explicit button; nothing is deleted on a single tap.
 */
export function ConfirmButton({
  label,
  confirmLabel,
  question,
  onConfirm,
  size = "md",
  wide,
}: {
  label: string;
  confirmLabel: string;
  /** Says what will be lost, e.g. "Delete the song and both takes?" */
  question?: string;
  onConfirm: () => Promise<unknown> | void;
  size?: "sm" | "md";
  wide?: boolean;
}) {
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  if (!asking) {
    return (
      <Button variant="danger" size={size} wide={wide} onClick={() => setAsking(true)}>
        {label}
      </Button>
    );
  }
  return (
    <Stack gap={2}>
      {question ? (
        <Text variant="small" tone="dim">
          {question}
        </Text>
      ) : null}
      <Inline gap={2}>
        <Button size={size} onClick={() => setAsking(false)} disabled={busy}>
          Cancel
        </Button>
        <div className="grow">
          <Button
            variant="danger"
            size={size}
            wide
            loading={busy}
            onClick={async () => {
              setBusy(true);
              try {
                await onConfirm();
              } finally {
                setBusy(false);
                setAsking(false);
              }
            }}
          >
            {confirmLabel}
          </Button>
        </div>
      </Inline>
    </Stack>
  );
}

/**
 * The toasts from `toast()` (lib/toast.ts): floating at the top of the
 * screen, over whatever is there, so a confirmation or a failure is seen
 * however far the page is scrolled. A tap dismisses one. Mounted once, at
 * the root.
 */
export function Toaster() {
  const toasts = useSyncExternalStore(subscribeToasts, currentToasts);
  return createPortal(
    <div className="toaster" aria-live="polite">
      {toasts.map((entry) => (
        <button
          key={entry.id}
          type="button"
          className="toast"
          data-tone={entry.tone}
          role={entry.tone === "error" ? "alert" : "status"}
          onClick={() => dismissToast(entry.id)}
        >
          {entry.text}
        </button>
      ))}
    </div>,
    document.body,
  );
}
