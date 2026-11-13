import React from "react";
import ReactDOM from "react-dom/client";

import "./design";
import App from "./App";
import { watchKeyboard } from "./lib/keyboard";
import { initTheme } from "./lib/theme";
import "./styles.css";

initTheme();
watchKeyboard();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
