/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** The commit a CI build was made from (see build-info.ts). */
  readonly VITE_BUILD_COMMIT?: string;
  /** When a CI build was made, ISO 8601. */
  readonly VITE_BUILD_TIME?: string;
}
