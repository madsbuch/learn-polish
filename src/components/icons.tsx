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
