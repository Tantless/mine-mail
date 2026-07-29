import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const styles = readFileSync(resolve(process.cwd(), "src/styles.css"), "utf8");

function declarationsFor(selectorPattern) {
  return styles.match(new RegExp(`${selectorPattern}\\s*\\{([^}]*)\\}`, "s"))?.[1];
}

function nestedBlockFor(header) {
  const start = styles.indexOf(header);
  if (start < 0) return undefined;
  const openingBrace = styles.indexOf("{", start + header.length);
  if (openingBrace < 0) return undefined;

  let depth = 1;
  for (let index = openingBrace + 1; index < styles.length; index += 1) {
    if (styles[index] === "{") depth += 1;
    if (styles[index] === "}") depth -= 1;
    if (depth === 0) return styles.slice(openingBrace + 1, index);
  }
  return undefined;
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

describe("brand avatar sizing policy", () => {
  it("shares the reader mark proportions with compact avatars", () => {
    expect(
      declarationsFor("\\.mail-row__avatar\\.profile-avatar--brand"),
    ).toBeUndefined();
    expect(styles).not.toMatch(
      /\.mail-row__avatar\.profile-avatar--(?:google|openrouter|figma|microsoft)/,
    );
    expect(declarationsFor("\\.account-card__avatar")).toMatch(
      /^\s*--brand-letter-size:\s*12px;/m,
    );
    expect(declarationsFor("\\.brand-mark__letters")).toMatch(
      /font-size:\s*var\(--brand-letter-size,\s*1\.35em\)/,
    );
  });
});

describe("embedded settings stylesheet boundary", () => {
  it("removes the retired modal shell and keeps one canonical base rule", () => {
    [
      "settings-layer",
      "settings-panel",
      "settings-brand",
      "settings-heading-with-icon",
      "settings-status-chip",
      "settings-account-card__actions",
      "settings-account-card__avatar-action",
      "settings-subsection--account-form",
      "settings-text-button",
      "settings-footer",
    ].forEach((className) => {
      expect(styles).not.toContain(`.${className}`);
    });

    [
      "settings-sidebar",
      "settings-nav",
      "settings-content",
      "settings-scroll",
      "settings-page",
      "settings-account-card",
      "settings-preference-row",
    ].forEach((className) => {
      expect(
        styles.match(new RegExp(`^\\.${className}\\s*\\{`, "gm")),
      ).toHaveLength(1);
    });
  });
});

describe("compose page and stationery policy", () => {
  it("keeps the solid compose page and the original rounded editor surface", () => {
    const panel = declarationsFor("(?:^|\\r?\\n)\\.compose-panel");
    expect(panel).toMatch(/background:\s*var\(--compose-page-surface\)/);
    expect(panel).not.toMatch(/backdrop-filter/);

    const fields = declarationsFor("\\.compose-fields");
    expect(fields).toMatch(/border:\s*0/);
    expect(fields).toMatch(/background:\s*transparent/);

    const editorShell = declarationsFor("\\.compose-editor-shell");
    expect(editorShell).toMatch(/border:\s*1px solid var\(--compose-divider\)/);
    expect(editorShell).toMatch(/border-radius:\s*12px/);
    expect(editorShell).toMatch(
      /background:\s*var\(--compose-content-surface\)/,
    );
    expect(editorShell).toMatch(/overflow:\s*hidden/);

    const editorFocus = declarationsFor("\\.compose-editor-shell:focus-within");
    expect(editorFocus).toMatch(/border-color:/);
    expect(editorFocus).toMatch(/box-shadow:/);

    const proseMirrorFocus = declarationsFor(
      "\\.compose-rich-editor > \\.ProseMirror:focus-visible",
    );
    expect(proseMirrorFocus).toMatch(/box-shadow:\s*none/);
  });

  it("keeps paper rules on the scrolling editor and frames grid paper", () => {
    const linedEditor = declarationsFor(
      '\\.compose-editor-shell\\[data-stationery="lined"\\]\\s*\\.compose-rich-editor\\s*>\\s*\\.ProseMirror',
    );
    expect(linedEditor).toMatch(/background-attachment:\s*local,\s*local/);
    expect(linedEditor).toMatch(
      /var\(--compose-paper-surface\)\s+calc\(var\(--compose-paper-block-overhang\) \+ 1px\)/,
    );

    const gridEditor = declarationsFor(
      '\\.compose-editor-shell\\[data-stationery="grid"\\]\\s*\\.compose-rich-editor\\s*>\\s*\\.ProseMirror',
    );
    expect(gridEditor).toMatch(/background-attachment:\s*local,\s*local/);
    expect(gridEditor).toMatch(
      /box-shadow:\s*inset 0 0 0 1px var\(--compose-paper-rule\)/,
    );

    const paperSurface = declarationsFor(
      '\\.compose-editor-shell\\[data-stationery="lined"\\],\\s*\\.compose-editor-shell\\[data-stationery="grid"\\]',
    );
    expect(paperSurface).toMatch(
      /background:\s*var\(--compose-paper-surface\)/,
    );
    expect(paperSurface).not.toMatch(/border(?:-radius)?:/);
    expect(paperSurface).not.toMatch(/margin(?:-top|-bottom)?:/);

    const editor = declarationsFor("\\.compose-rich-editor");
    expect(editor).toMatch(
      /var\(--compose-editor-text-block-inset\)\s+var\(--compose-editor-text-inline-inset\) 28px/,
    );

    const gridToken = declarationsFor(
      '\\.compose-editor-shell\\[data-stationery="grid"\\] \\.compose-grid-cell-token',
    );
    expect(gridToken).toMatch(/width:\s*var\(--compose-paper-cell-size\)/);
    expect(gridToken).toMatch(/min-width:\s*var\(--compose-paper-cell-size\)/);
    expect(gridToken).toMatch(/max-width:\s*var\(--compose-paper-cell-size\)/);
    expect(gridToken).toMatch(/justify-content:\s*center/);
    expect(gridToken).toMatch(/text-indent:\s*0/);
    expect(gridToken).toMatch(/white-space:\s*pre/);

    const ordinaryIndent = declarationsFor(
      '\\.compose-rich-editor\\s*\\.ProseMirror\\s*p\\[data-first-line-indent="tab"\\]',
    );
    expect(ordinaryIndent).toMatch(/text-indent:\s*2em/);

    const gridIndent = declarationsFor(
      '\\.compose-editor-shell\\[data-stationery="grid"\\]\\s*\\.compose-rich-editor\\s*\\.ProseMirror\\s*p\\[data-first-line-indent="tab"\\]',
    );
    expect(gridIndent).toMatch(
      /text-indent:\s*calc\(var\(--compose-paper-cell-size\) \* 2\)/,
    );
  });

  it("keeps compact compose listboxes visible inside the formatting row", () => {
    const toolbar = declarationsFor("\\.compose-format-toolbar");
    expect(toolbar).toMatch(/overflow:\s*visible/);
    expect(toolbar).not.toMatch(/overflow-x:\s*auto/);

    const trigger = declarationsFor(
      "\\.compose-format-select \\.themed-select__trigger",
    );
    expect(trigger).toMatch(/height:\s*30px/);
    expect(trigger).toMatch(/font-size:\s*11px/);

    const menu = declarationsFor(
      "\\.compose-format-select \\.themed-select__menu",
    );
    expect(menu).toMatch(/right:\s*auto/);
    expect(menu).toMatch(/left:\s*0/);
    expect(menu).toMatch(/width:\s*104px/);
  });

  it("renders semantic italic text and a zero-width stored-format caret probe", () => {
    const italic = declarationsFor(
      "\\.compose-rich-editor \\.ProseMirror em,\\s*\\.compose-rich-editor \\.ProseMirror i",
    );
    expect(italic).toMatch(/font-style:\s*italic/);

    const caretProbe = declarationsFor(
      "\\.compose-rich-editor \\.compose-format-caret-probe",
    );
    expect(caretProbe).toMatch(/display:\s*inline !important/);
    expect(caretProbe).toMatch(/width:\s*0 !important/);
    expect(caretProbe).toMatch(/height:\s*0 !important/);
  });
});

describe("release-state semantic styling", () => {
  it("derives shared state surfaces from semantic theme roles", () => {
    const root = declarationsFor("(?:^|\\r?\\n):root");
    expect(root).toMatch(
      /--state-info-surface:\s*color-mix\([\s\S]*?var\(--color-primary\)/,
    );
    expect(root).toMatch(
      /--state-success-surface:\s*color-mix\([\s\S]*?var\(--color-success\)/,
    );
    expect(root).toMatch(
      /--state-warning-surface:\s*color-mix\([\s\S]*?var\(--color-warning\)/,
    );
    expect(root).toMatch(
      /--state-danger-surface:\s*color-mix\([\s\S]*?var\(--color-danger\)/,
    );
  });

  it.each(["daylight", "night", "dusk", "forest"])(
    "keeps %s compose and paper surfaces theme-owned",
    (theme) => {
      const themeBlock = declarationsFor(
        `:root\\[data-theme="${theme}"\\]`,
      );
      expect(themeBlock).toMatch(/--compose-page-surface:/);
      expect(themeBlock).toMatch(/--compose-paper-surface:/);
      expect(themeBlock).toMatch(/--compose-paper-rule:/);
    },
  );

  it("styles reader attachment lifecycle and feedback without a white-only palette", () => {
    expect(
      declarationsFor('\\.attachment-card\\[data-state="saved"\\]'),
    ).toMatch(/background:\s*var\(--state-success-surface\)/);
    expect(
      declarationsFor('\\.attachment-card\\[data-state="error"\\]'),
    ).toMatch(/background:\s*var\(--state-danger-surface\)/);
    expect(
      declarationsFor('\\.attachment-card\\[aria-busy="true"\\],\\s*\\.attachment-card\\[data-state="saving"\\]'),
    ).toMatch(/background:\s*var\(--state-info-surface\)/);
    expect(declarationsFor("\\.delivery-status")).toMatch(
      /background:\s*var\(--state-info-surface\)/,
    );
  });

  it("gives compose attachments bounded overflow and immutable context hierarchy", () => {
    expect(declarationsFor("\\.compose-attachments")).toMatch(
      /max-height:\s*min\(26vh,\s*210px\)/,
    );
    expect(declarationsFor("\\.compose-attachments")).toMatch(
      /overflow-y:\s*auto/,
    );
    expect(declarationsFor("\\.compose-attachment")).toMatch(
      /grid-template-columns:\s*34px minmax\(0,\s*1fr\) 44px/,
    );
    expect(
      declarationsFor(
        '\\.compose-forward-context\\[data-immutable="true"\\]',
      ),
    ).toMatch(/box-shadow:\s*inset 3px 0 0/);
    expect(
      declarationsFor("\\.compose-forward-context__identity dd"),
    ).toMatch(/overflow-wrap:\s*anywhere/);
  });

  it("distinguishes pagination, capability and pending-folder states", () => {
    for (const state of [
      "loading",
      "retry",
      "offline",
      "unavailable",
      "complete",
    ]) {
      expect(
        declarationsFor(
          `\\.mail-load-progress\\[data-pagination-state="${state}"\\]`,
        ),
      ).toBeDefined();
    }
    expect(
      declarationsFor(
        '\\.empty-list\\[data-empty-state="capability"\\]',
      ),
    ).toMatch(/background:\s*var\(--state-warning-surface\)/);
    expect(
      declarationsFor(
        '\\.folder-nav__item\\[data-capability-status="needs_creation_confirmation"\\]',
      ),
    ).toMatch(/var\(--color-warning\)/);
    expect(
      declarationsFor('\\.folder-nav__item\\[data-pending-count\\]'),
    ).toBeDefined();
  });
});

describe("release-state accessibility and reflow contracts", () => {
  it("switches to the compact three-column geometry at the 1250px boundary", () => {
    const fullDesktop = declarationsFor("(?:^|\\r?\\n):root");
    const compactDesktop = nestedBlockFor("@media (max-width: 1250px)");

    expect(fullDesktop).toMatch(
      /--sidebar-width:\s*clamp\(260px,\s*20\.5vw,\s*340px\)/,
    );
    expect(fullDesktop).toMatch(
      /--mail-list-width:\s*clamp\(390px,\s*29vw,\s*486px\)/,
    );
    expect(fullDesktop).toMatch(/--reader-min-width:\s*480px/);
    expect(compactDesktop).toMatch(
      /:root\s*\{[\s\S]*--sidebar-width:\s*78px/,
    );
    expect(compactDesktop).toMatch(
      /:root\s*\{[\s\S]*--mail-list-width:\s*clamp\(350px,\s*35vw,\s*420px\)/,
    );
    expect(compactDesktop).toMatch(
      /:root\s*\{[\s\S]*--reader-min-width:\s*420px/,
    );
    expect(compactDesktop).toMatch(
      /\.brand__name,[\s\S]*\.sidebar-action span\s*\{[\s\S]*display:\s*none/,
    );
  });

  it("keeps consequential confirmations theme-owned and safely operable", () => {
    expect(
      declarationsFor("\\.consequential-confirm-dialog"),
    ).toMatch(/width:\s*min\(450px,\s*calc\(100vw - 34px\)\)/);
    expect(
      declarationsFor(
        "\\.confirm-dialog > \\.consequential-confirm-dialog__note",
      ),
    ).toMatch(/background:\s*var\(--state-warning-surface\)/);
    expect(
      declarationsFor(
        "\\.confirm-dialog > \\.consequential-confirm-dialog__error",
      ),
    ).toMatch(/background:\s*var\(--state-danger-surface\)/);
    expect(declarationsFor("\\.confirm-dialog__danger-action")).toMatch(
      /color:\s*var\(--color-on-danger\)/,
    );
    expect(declarationsFor("\\.danger-button")).toMatch(
      /min-height:\s*40px/,
    );
  });

  it("uses theme-owned action contrast and keeps key targets operable", () => {
    expect(declarationsFor("\\.send-button")).toMatch(
      /color:\s*var\(--color-on-primary\)/,
    );
    expect(declarationsFor("\\.recipient-toggle")).toMatch(
      /min-height:\s*40px/,
    );
    expect(declarationsFor("\\.star-button")).toMatch(
      /width:\s*40px[\s\S]*height:\s*40px/,
    );
    expect(declarationsFor("\\.mail-tab")).toMatch(
      /min-width:\s*40px[\s\S]*min-height:\s*40px/,
    );
    expect(
      declarationsFor("\\.folder-nav__item,\\s*\\.sidebar-action"),
    ).toMatch(/min-height:\s*44px/);
    expect(declarationsFor("\\.secondary-button,\\s*\\.draft-button")).toMatch(
      /min-height:\s*40px/,
    );
    expect(declarationsFor("\\.send-button")).toMatch(
      /min-height:\s*40px/,
    );
    expect(declarationsFor("\\.settings-help__button")).toMatch(
      /width:\s*40px[\s\S]*height:\s*40px/,
    );
    expect(
      declarationsFor("\\.compose-stationery-toggle\\.icon-button"),
    ).toMatch(/height:\s*34px/);
    expect(declarationsFor("\\.compose-icon-segment")).toMatch(
      /border-radius:\s*999px/,
    );
    expect(declarationsFor("\\.compose-save-state")).toMatch(
      /width:\s*74px[\s\S]*flex:\s*0 0 74px/,
    );
    expect(styles).not.toMatch(
      /\.reader-toolbar__group:first-child\s+\.icon-button:nth-of-type\(4\)/,
    );
  });

  it("moves one theme-owned folder selection surface within normal motion", () => {
    expect(declarationsFor("\\.folder-nav")).toMatch(
      /position:\s*relative[\s\S]*isolation:\s*isolate/,
    );
    expect(declarationsFor("\\.folder-nav__selection")).toMatch(
      /background:\s*var\(--sidebar-selected\)/,
    );
    expect(declarationsFor("\\.folder-nav__selection")).toMatch(
      /transform:\s*translate3d\([\s\S]*--folder-selection-x[\s\S]*--folder-selection-y/,
    );
    expect(declarationsFor("\\.folder-nav__selection")).toMatch(
      /transform var\(--motion-normal\)/,
    );
    expect(
      declarationsFor('\\.folder-nav__item\\[data-selected="true"\\]'),
    ).toMatch(/background:\s*transparent/);
  });

  it("keeps native list actions focus-visible above the row overlay", () => {
    expect(declarationsFor("\\.mail-row__open")).toMatch(
      /min-height:\s*44px/,
    );
    expect(
      declarationsFor(
        "\\.mail-row:has\\(\\.mail-row__open:focus-visible\\)",
      ),
    ).toMatch(/box-shadow:\s*var\(--focus-ring\)/);
    expect(declarationsFor("\\.reader-toolbar \\.icon-button")).toMatch(
      /width:\s*44px[\s\S]*height:\s*44px/,
    );
    expect(
      declarationsFor("\\.compose-attachment__remove\\.icon-button"),
    ).toMatch(/width:\s*44px[\s\S]*height:\s*44px/);
  });

  it("keeps programmatic dialog focus visible and live-only copy off canvas", () => {
    expect(declarationsFor("\\.confirm-dialog:focus-visible")).toMatch(
      /box-shadow:\s*var\(--focus-ring\)/,
    );
    expect(declarationsFor("\\.sr-only")).toMatch(
      /clip-path:\s*inset\(50%\)/,
    );
    expect(declarationsFor("\\.sr-only")).not.toMatch(/display:\s*none/);
  });

  it("covers compact desktop, single-pane, reduced-motion and forced-color modes", () => {
    const compact = nestedBlockFor("@media (max-width: 940px)");
    const singlePane = nestedBlockFor("@media (max-width: 720px)");
    const reducedMotion = nestedBlockFor(
      "@media (prefers-reduced-motion: reduce)",
    );
    const forcedColors = nestedBlockFor("@media (forced-colors: active)");

    expect(compact).toMatch(/\.compose-attachments__list[\s\S]*grid-template-columns:\s*minmax\(0,\s*1fr\)/);
    expect(singlePane).toMatch(
      /\.compose-attachments[\s\S]*margin-inline:\s*12px/,
    );
    expect(singlePane).toMatch(
      /\.compose-footer[\s\S]*flex-wrap:\s*wrap/,
    );
    expect(reducedMotion).toMatch(
      /\.attachment-card\[aria-busy="true"\][\s\S]*animation:\s*none !important/,
    );
    expect(reducedMotion).toMatch(
      /\.folder-nav__selection[\s\S]*will-change:\s*auto/,
    );
    expect(forcedColors).toMatch(
      /\.folder-nav__selection[\s\S]*background:\s*Highlight/,
    );
    expect(forcedColors).toMatch(
      /\.compose-forward-context\[data-immutable="true"\][\s\S]*forced-color-adjust:\s*auto/,
    );
  });
});
