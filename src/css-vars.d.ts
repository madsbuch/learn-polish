import "react";

declare module "react" {
  /** CSS custom properties ("--value": 40) in a style object. */
  interface CSSProperties {
    [property: `--${string}`]: string | number | undefined;
  }
}
