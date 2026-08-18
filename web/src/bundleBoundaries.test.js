import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const source = (path) =>
  readFileSync(resolve(process.cwd(), path), "utf8").replaceAll("\r\n", "\n");

describe("production bundle boundaries", () => {
  it("keeps the cached mailbox path eager and secondary workspaces lazy", () => {
    const app = source("src/App.jsx");

    expect(app).toContain(
      'import { emptyCompose } from "./models/compose.js";',
    );
    expect(app).toContain(
      'import { MailList } from "./components/MailList.jsx";',
    );
    expect(app).toContain(
      'import { MessageView } from "./components/MessageView.jsx";',
    );
    expect(app).toMatch(
      /const ContactsWorkspace = lazy\(\(\) =>\s*import\("\.\/components\/ContactsWorkspace\.jsx"\)/,
    );
    expect(app).toMatch(
      /const SettingsPanel = lazy\(\(\) =>\s*import\("\.\/components\/SettingsPanel\.jsx"\)/,
    );
    expect(app).not.toContain("./data/mockMail.js");
  });

  it("loads the rich editor only after the compose surface is opened", () => {
    const composePanel = source("src/components/ComposePanel.jsx");

    expect(composePanel).toMatch(
      /const RichTextEditor = lazy\(\(\) =>\s*import\("\.\/RichTextEditor\.jsx"\)/,
    );
    expect(composePanel).not.toContain(
      'import { RichTextEditor } from "./RichTextEditor.jsx";',
    );
  });

  it("keeps demo fixtures behind the explicit demo adapter", () => {
    const mailApi = source("src/services/mailApi.js");
    const demoAdapter = source("src/services/demoMailAdapter.js");

    expect(mailApi).toContain('? import("./demoMailAdapter.js")');
    expect(mailApi).not.toContain("../data/demoMail.js");
    expect(demoAdapter).toContain(
      'import { demoDrafts, demoMessages } from "../data/demoMail.js";',
    );
  });

  it("opens the explicit WebUI demo without changing the Tauri dev entry", () => {
    const packageJson = JSON.parse(source("package.json"));
    const tauriConfig = JSON.parse(source("src-tauri/tauri.conf.json"));

    expect(packageJson.scripts.dev).toBe("vite --mode demo --open");
    expect(packageJson.scripts["dev:tauri"]).toBe("vite");
    expect(tauriConfig.build.beforeDevCommand).toBe("npm run dev:tauri");
  });

  it("separates the eager React runtime without changing mailbox loading", () => {
    const viteConfig = source("vite.config.mjs");

    expect(viteConfig).toContain('return "react-runtime";');
    expect(viteConfig).toContain('/node_modules/react-dom/');
  });
});
