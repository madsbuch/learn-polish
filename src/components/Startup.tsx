import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

import { Button, Spinner, Stack, Text } from "../design";

/** One step of starting up, shown while the app loads. */
export interface BootStep {
  /** What the step is, and the command behind it (for tracing). */
  label: string;
  command: string;
  state: "pending" | "done" | "failed";
  startedAt: number;
  /** How long it took, once it finished. */
  ms?: number;
  error?: string;
}

/**
 * The loading screen, with what it is waiting for: each startup step with
 * how long it has taken, a failure's message, and the app's version — so a
 * screenshot of a stall says where it stalled. A retry once something
 * failed or nothing has moved for a while.
 */
export function Startup({ steps, onRetry }: { steps: BootStep[]; onRetry: () => void }) {
  const [now, setNow] = useState(Date.now());
  const [version, setVersion] = useState("");
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    getVersion().then(setVersion).catch(() => {});
    return () => window.clearInterval(timer);
  }, []);
  const failed = steps.some((step) => step.state === "failed");
  const slow = steps.some((step) => step.state === "pending" && now - step.startedAt > 10_000);

  return (
    <div className="center-screen">
      <Stack gap={4} align="center">
        {failed ? null : <Spinner />}
        <Text tone="dim">{failed ? "Something went wrong while starting" : "Przygotowuję…"}</Text>
        <div className="boot-steps">
          {steps.map((step) => (
            <div key={step.command} className="boot-step" data-state={step.state}>
              <span>{step.state === "done" ? "✓" : step.state === "failed" ? "✗" : "…"}</span>
              <span className="grow">
                {step.label} <code>{step.command}</code>
                {step.error ? <span className="boot-step__error">{step.error}</span> : null}
              </span>
              <span>
                {step.ms !== undefined
                  ? `${(step.ms / 1000).toFixed(1)} s`
                  : `${Math.floor((now - step.startedAt) / 1000)} s`}
              </span>
            </div>
          ))}
        </div>
        {slow && !failed ? (
          <Text variant="small" tone="dim" align="center">
            This is taking longer than it should. A screenshot of this screen shows where it is stuck.
          </Text>
        ) : null}
        {failed || slow ? <Button onClick={onRetry}>Try again</Button> : null}
        <Text variant="caption" tone="faint">
          {version ? `Polski ${version}` : ""}
        </Text>
      </Stack>
    </div>
  );
}
