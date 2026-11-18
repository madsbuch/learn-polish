import React from "react";
import ReactDOM from "react-dom/client";

import "./design";
import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { Toaster } from "./design";
import { watchKeyboard } from "./lib/keyboard";
import { logUncaught } from "./lib/log";
import { initTheme } from "./lib/theme";
import "./styles.css";

logUncaught();
initTheme();
watchKeyboard();

const root = document.getElementById("root");
if (!root) throw new Error("index.html has no #root element.");
ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
    <Toaster />
  </React.StrictMode>,
);
