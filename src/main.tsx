import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import "./i18n";
import "./styles.css";

const root = document.getElementById("root");

if (root === null) {
  throw new Error("Koma root element is missing");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
