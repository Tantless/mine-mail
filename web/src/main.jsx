import React from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/inter";
import "@fontsource-variable/nunito";
import { App } from "./App.jsx";
import { NewMailNotification } from "./components/NewMailNotification.jsx";
import "./styles.css";

const notificationSurface =
  new URLSearchParams(window.location.search).get("surface") ===
  "new-mail-notification";
if (notificationSurface) {
  document.documentElement.dataset.surface = "new-mail-notification";
}
const RootSurface = notificationSurface ? NewMailNotification : App;

createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    <RootSurface />
  </React.StrictMode>,
);
