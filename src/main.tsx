import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import "./i18n";
import "./styles.css";

const isIPad =
  /\biPad\b/i.test(navigator.userAgent) ||
  (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);

if (isIPad) {
  document.documentElement.dataset.formFactor = "ipad";
}

const root = document.getElementById("root");

if (root === null) {
  throw new Error("Koma root element is missing");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
