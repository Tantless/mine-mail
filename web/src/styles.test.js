import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const styles = readFileSync(resolve(process.cwd(), "src/styles.css"), "utf8");

function declarationsFor(selectorPattern) {
  return styles.match(new RegExp(`${selectorPattern}\\s*\\{([^}]*)\\}`, "s"))?.[1];
}

describe("text selection policy", () => {
  it("keeps app chrome inert while allowing text entry and opened mail content", () => {
    expect(declarationsFor("\\.app-shell")).toMatch(
      /^\s*user-select:\s*none;/m,
    );
    expect(
      declarationsFor(
        '\\.app-shell input,\\s*\\.app-shell textarea,\\s*\\.app-shell \\[contenteditable="true"\\],\\s*\\.reader-panel--message \\.reader-scroll',
      ),
    ).toMatch(/^\s*user-select:\s*text;/m);
    expect(
      declarationsFor(
        '\\.reader-panel--message :is\\(button, \\[role="button"\\], summary\\)',
      ),
    ).toMatch(/^\s*user-select:\s*none;/m);
  });
});
