import type { ReactNode } from "react";

/**
 * The app mark: a speech bubble carrying Ł, the letter only Polish has.
 * Same artwork as public/icon.svg (which the app icons are rendered from),
 * minus the drop shadow so it stays crisp at small sizes.
 */
export function Logo({ size = 36 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 1024 1024" aria-hidden="true">
      <defs>
        <linearGradient id="logo-bg" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#7a5cff" />
          <stop offset="0.55" stopColor="#5236e0" />
          <stop offset="1" stopColor="#2c1a8c" />
        </linearGradient>
        <linearGradient id="logo-red" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#f43f5e" />
          <stop offset="1" stopColor="#d4103a" />
        </linearGradient>
      </defs>
      <rect width="1024" height="1024" rx="232" fill="url(#logo-bg)" />
      <path
        fill="#fff"
        d="M362 228h300a150 150 0 0 1 150 150v188a150 150 0 0 1-150 150H448l-118 98c-14 12-35 1-32-17l14-92A150 150 0 0 1 212 566V378a150 150 0 0 1 150-150z"
      />
      <g fill="url(#logo-red)">
        <rect x="440" y="326" width="92" height="300" rx="20" />
        <rect x="440" y="542" width="226" height="84" rx="20" />
        <rect x="386" y="430" width="200" height="62" rx="31" transform="rotate(-30 486 461)" />
      </g>
    </svg>
  );
}

function Glyph({ children }: { children: ReactNode }) {
  return (
    <svg
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.9"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

export const NavIcon = {
  songs: () => (
    <Glyph>
      <path d="M9 18V5l11-2v13" />
      <circle cx="6" cy="18" r="3" />
      <circle cx="17" cy="16" r="3" />
    </Glyph>
  ),
  today: () => (
    <Glyph>
      <path d="M4 10.5 12 4l8 6.5V19a1 1 0 0 1-1 1h-4.5v-5.5h-5V20H5a1 1 0 0 1-1-1z" />
    </Glyph>
  ),
  library: () => (
    <Glyph>
      <path d="M4 5.5A1.5 1.5 0 0 1 5.5 4H10a2 2 0 0 1 2 2v14a2 2 0 0 0-2-2H4z" />
      <path d="M20 5.5A1.5 1.5 0 0 0 18.5 4H14a2 2 0 0 0-2 2v14a2 2 0 0 1 2-2h6z" />
    </Glyph>
  ),
  tutor: () => (
    <Glyph>
      <path d="M12 3.5l1.9 4.6 4.6 1.9-4.6 1.9-1.9 4.6-1.9-4.6-4.6-1.9 4.6-1.9z" />
      <path d="M18.5 15.5l.8 1.9 1.9.8-1.9.8-.8 1.9-.8-1.9-1.9-.8 1.9-.8z" />
    </Glyph>
  ),
  progress: () => (
    <Glyph>
      <path d="M4 20V10M10 20V4M16 20v-7M22 20H2" />
    </Glyph>
  ),
  settings: () => (
    <Glyph>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
    </Glyph>
  ),
};

function Solid({ children, size = 22 }: { children: ReactNode; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      {children}
    </svg>
  );
}

export const PlayerIcon = {
  play: ({ size }: { size?: number }) => (
    <Solid size={size}>
      <path d="M8 5.2v13.6a1 1 0 0 0 1.5.86l11-6.8a1 1 0 0 0 0-1.72l-11-6.8A1 1 0 0 0 8 5.2z" />
    </Solid>
  ),
  pause: ({ size }: { size?: number }) => (
    <Solid size={size}>
      <rect x="6" y="5" width="4.2" height="14" rx="1.2" />
      <rect x="13.8" y="5" width="4.2" height="14" rx="1.2" />
    </Solid>
  ),
  next: () => (
    <Solid>
      <path d="M5 6.2v11.6a.9.9 0 0 0 1.4.75L15 13v4.5a1 1 0 0 0 2 0v-11a1 1 0 0 0-2 0V11L6.4 5.45A.9.9 0 0 0 5 6.2z" />
    </Solid>
  ),
  previous: () => (
    <Solid>
      <path d="M19 6.2v11.6a.9.9 0 0 1-1.4.75L9 13v4.5a1 1 0 0 1-2 0v-11a1 1 0 0 1 2 0V11l8.6-5.55A.9.9 0 0 1 19 6.2z" />
    </Solid>
  ),
  repeat: () => (
    <Glyph>
      <path d="M17 2l3 3-3 3" />
      <path d="M4 11V9a4 4 0 0 1 4-4h12" />
      <path d="M7 22l-3-3 3-3" />
      <path d="M20 13v2a4 4 0 0 1-4 4H4" />
    </Glyph>
  ),
  speaker: () => (
    <Glyph>
      <rect x="5" y="2.5" width="14" height="19" rx="2.5" />
      <circle cx="12" cy="14.5" r="3.5" />
      <circle cx="12" cy="7" r="1" />
    </Glyph>
  ),
  shuffle: () => (
    <Glyph>
      <path d="M16 3h5v5" />
      <path d="M4 20 21 3" />
      <path d="M21 16v5h-5" />
      <path d="M15 15l6 6" />
      <path d="M4 4l5 5" />
    </Glyph>
  ),
  addToPlaylist: () => (
    <Glyph>
      <path d="M3 6h12M3 12h12M3 18h7" />
      <path d="M18 14v7M14.5 17.5h7" />
    </Glyph>
  ),
  share: () => (
    <Glyph>
      <circle cx="18" cy="5" r="2.6" />
      <circle cx="6" cy="12" r="2.6" />
      <circle cx="18" cy="19" r="2.6" />
      <path d="M8.3 10.8l7.4-4.4M8.3 13.2l7.4 4.4" />
    </Glyph>
  ),
  download: () => (
    <Glyph>
      <path d="M12 3v12" />
      <path d="M7 10.5l5 5 5-5" />
      <path d="M4 20h16" />
    </Glyph>
  ),
  trash: () => (
    <Glyph>
      <path d="M4 7h16M9.5 7V4.5h5V7M6.5 7l1 13h9l1-13" />
    </Glyph>
  ),
};

/** The back arrow on full screens. */
export function BackIcon() {
  return (
    <Glyph>
      <path d="M15 5l-7 7 7 7" />
    </Glyph>
  );
}

/** Closes a screen that slid up, like the player. */
export function DownIcon() {
  return (
    <Glyph>
      <path d="M5 9l7 7 7-7" />
    </Glyph>
  );
}

/** Marks a row that opens another screen. */
export function ChevronIcon() {
  return (
    <Glyph>
      <path d="M9 5l7 7-7 7" />
    </Glyph>
  );
}

export function CloseIcon() {
  return (
    <Glyph>
      <path d="M6 6l12 12M18 6L6 18" />
    </Glyph>
  );
}

export function SpeakerIcon() {
  return (
    <Glyph>
      <path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" />
      <path d="M15.5 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11" />
    </Glyph>
  );
}

export function MutedIcon() {
  return (
    <Glyph>
      <path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" />
      <path d="M16 9.5l5 5M21 9.5l-5 5" />
    </Glyph>
  );
}

export function SearchIcon() {
  return (
    <Glyph>
      <circle cx="11" cy="11" r="6.5" />
      <path d="M16 16l4.5 4.5" />
    </Glyph>
  );
}

export function PlusIcon() {
  return (
    <Glyph>
      <path d="M12 5v14M5 12h14" />
    </Glyph>
  );
}

export function MoreIcon() {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <circle cx="5.5" cy="12" r="1.8" />
      <circle cx="12" cy="12" r="1.8" />
      <circle cx="18.5" cy="12" r="1.8" />
    </svg>
  );
}

export function SendIcon() {
  return (
    <Glyph>
      <path d="M5 12h13M13 6l6 6-6 6" />
    </Glyph>
  );
}

export function ArrowUpIcon() {
  return (
    <Glyph>
      <path d="M12 19V5M6 11l6-6 6 6" />
    </Glyph>
  );
}

export function ArrowDownIcon() {
  return (
    <Glyph>
      <path d="M12 5v14M6 13l6 6 6-6" />
    </Glyph>
  );
}

export function CheckIcon() {
  return (
    <Glyph>
      <path d="M5 12.5l4.5 4.5L19 7.5" />
    </Glyph>
  );
}

/** A drill block: quick, mechanical practice. */
export function DrillIcon() {
  return (
    <Glyph>
      <path d="M13 3L5 13.5h6L10 21l8-10.5h-6z" />
    </Glyph>
  );
}

/** A study block: a lesson to read, then a check-in. */
export function StudyIcon() {
  return (
    <Glyph>
      <path d="M4 5.5A1.5 1.5 0 0 1 5.5 4H10a2 2 0 0 1 2 2v14a2 2 0 0 0-2-2H4z" />
      <path d="M20 5.5A1.5 1.5 0 0 0 18.5 4H14a2 2 0 0 0-2 2v14a2 2 0 0 1 2-2h6z" />
    </Glyph>
  );
}
