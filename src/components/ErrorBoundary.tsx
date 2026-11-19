import { Component, type ErrorInfo, type ReactNode } from "react";

import { Banner, Button, Stack, Text } from "../design";
import { logCause } from "../lib/log";

/**
 * Catches an error while drawing a screen: it goes in the Diagnostics log,
 * and the screen says so and offers to try again, instead of the whole app
 * going blank.
 */
export class ErrorBoundary extends Component<{ children: ReactNode }, { failed: string | null }> {
  state: { failed: string | null } = { failed: null };

  static getDerivedStateFromError(error: unknown) {
    return { failed: error instanceof Error ? error.message : String(error) };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    const where =
      info.componentStack
        ?.split("\n")
        .find((line) => line.trim())
        ?.trim() ?? "";
    logCause("screen", error instanceof Error ? new Error(`${error.message}${where ? ` (${where})` : ""}`) : error);
  }

  render() {
    if (this.state.failed === null) return this.props.children;
    return (
      <div className="error-screen">
        <Stack gap={4}>
          <Text variant="title" as="h1">
            Something went wrong
          </Text>
          <Banner tone="error">{this.state.failed}</Banner>
          <Text tone="dim" as="p">
            It is in the log under Settings → Your data → Diagnostics. Try again, or restart the app.
          </Text>
          <Button variant="primary" wide onClick={() => this.setState({ failed: null })}>
            Try again
          </Button>
        </Stack>
      </div>
    );
  }
}
