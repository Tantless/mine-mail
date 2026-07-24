import React from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/nunito";
import InstallerApp from "./InstallerApp";
import "./installer.css";

createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    <InstallerApp />
  </React.StrictMode>,
);
