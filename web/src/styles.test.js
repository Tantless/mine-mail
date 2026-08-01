import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const styles = readFileSync(resolve(process.cwd(), "src/styles.css"), "utf8");
const contactsStyles = readFileSync(
  resolve(process.cwd(), "src/components/ContactsWorkspace.css"),
  "utf8",
);

function declarationsFor(selectorPattern) {
  return styles.match(new RegExp(`${selectorPattern}\\s*\\{([^}]*)\\}`, "s"))?.[1];
}

function contactDeclarationsFor(selectorPattern) {
  return contactsStyles.match(
    new RegExp(`${selectorPattern}\\s*\\{([^}]*)\\}`, "s"),
  )?.[1];
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

describe("isolated mail sizing contract", () => {
  it("uses the placeholder height only until the measured document is ready", () => {
    expect(declarationsFor("\\.html-message__document")).toMatch(
      /min-height:\s*220px/,
    );
    expect(
      declarationsFor('\\.html-message__document\\[data-ready="true"\\]'),
    ).toMatch(/min-height:\s*0/);
    expect(
      declarationsFor('\\.html-message__frame\\[data-ready="true"\\]'),
    ).toMatch(/min-height:\s*0/);
  });
});

describe("settings overlay positioning", () => {
  it("does not override fixed confirmation layers with workspace content positioning", () => {
    const settingsChildren = declarationsFor(
      "\\.settings-workspace > :not\\(\\.confirm-layer\\)",
    );
    const confirmLayer = declarationsFor("\\.confirm-layer");

    expect(settingsChildren).toMatch(/position:\s*relative/);
    expect(confirmLayer).toMatch(/position:\s*fixed/);
  });

  it("keeps account rows geometrically stable while remark fields open", () => {
    const accountCard = declarationsFor("\\.settings-account-card");
    const remarkEditor = declarationsFor("\\.settings-account-remark-editor");
    const remarkError = declarationsFor(
      "\\.settings-account-remark-editor__error",
    );
    expect(accountCard).toMatch(/height:\s*78px/);
    expect(remarkEditor).toMatch(/grid-column:\s*3/);
    expect(remarkEditor).toMatch(/min-height:\s*40px/);
    expect(remarkError).toMatch(/position:\s*absolute/);
    expect(styles).toMatch(
      /\.settings-account-remark-editor\s*\{\s*position:\s*absolute/,
    );
    expect(styles).not.toMatch(
      /\.settings-account-remark-editor\s*\{[^}]*grid-row:\s*2/,
    );
  });
});

describe("mail workspace motion contract", () => {
  it("uses center-anchored window motion without horizontal page travel", () => {
    const readerEntering = declarationsFor(
      '\\.reader-panel--message\\[data-reader-motion="entering"\\]',
    );
    const contactEntering = declarationsFor(
      '\\.contacts-detail-panel--selected\\[data-reader-motion="entering"\\]',
    );
    const readerWindowIn = nestedBlockFor("@keyframes reader-window-in");
    const listContextOut = nestedBlockFor("@keyframes mail-list-context-out");

    expect(readerEntering).toMatch(
      /animation:\s*reader-window-in var\(--motion-window\)/,
    );
    expect(readerEntering).toMatch(/transform-origin:\s*center/);
    expect(contactEntering).toMatch(
      /animation:\s*reader-window-in var\(--motion-window\)/,
    );
    expect(contactEntering).toMatch(/transform-origin:\s*center/);
    expect(readerWindowIn).toMatch(/scale\(0\.96\)/);
    expect(readerWindowIn).toMatch(/translate3d\(0,\s*6px,\s*0\)/);
    expect(readerWindowIn).not.toMatch(/translateX/);
    expect(listContextOut).toMatch(/scale\(0\.94\)/);
    expect(listContextOut).toMatch(/translate3d\(0,\s*0,\s*0\)/);
    expect(listContextOut).not.toMatch(/translateX/);
    expect(styles).not.toContain('data-reader-motion="swapping"');
    expect(styles).not.toContain("@keyframes reader-content-swap");
  });

  it("keeps collapse as one full window cycle and swaps folders at its midpoint", () => {
    const collapsing = declarationsFor(
      '\\.mail-workspace\\[data-list-motion="collapsing"\\] \\.mail-list-motion-frame',
    );
    const switchingOut = declarationsFor(
      '\\.mail-workspace\\[data-list-motion="switching-out"\\] \\.mail-list-motion-frame',
    );
    const switchingIn = declarationsFor(
      '\\.mail-workspace\\[data-list-motion="switching-in"\\] \\.mail-list-motion-frame',
    );
    const expanding = declarationsFor(
      '\\.mail-workspace\\[data-list-motion="expanding"\\] \\.mail-list-motion-frame',
    );
    const collapsedGeometry = declarationsFor(
      '\\.mail-workspace\\[data-list-motion="collapsing"\\],\\s*\\.mail-workspace\\[data-list-motion="collapsed"\\]',
    );
    const fastReaderExit = declarationsFor(
      '\\.reader-panel--message\\[data-reader-motion="exiting"\\]\\[data-reader-exit-speed="fast"\\]',
    );
    const fastContactExit = declarationsFor(
      '\\.contacts-detail-panel--selected\\[data-reader-motion="exiting"\\]\\[data-reader-exit-speed="fast"\\]',
    );
    const contactsRowsDuringSwitch = declarationsFor(
      '\\.mail-workspace\\[data-list-motion\\^="switching"\\] \\.contacts-row',
    );
    const contactsFallback = declarationsFor(
      "\\.mail-list-motion-frame > \\.secondary-workspace-loading",
    );

    expect(collapsing).toMatch(
      /animation:\s*mail-list-window-out var\(--motion-window\)/,
    );
    expect(switchingOut).toMatch(/var\(--motion-window-half\)/);
    expect(switchingIn).toMatch(/var\(--motion-window-half\)/);
    expect(expanding).toMatch(
      /mail-list-window-in var\(--motion-window-half\)[\s\S]*var\(--motion-window-half\) both/,
    );
    expect(collapsedGeometry).toMatch(
      /grid-template-columns:\s*0 minmax\(0,\s*1fr\)/,
    );
    expect(fastReaderExit).toMatch(
      /animation-duration:\s*var\(--motion-fast\)/,
    );
    expect(fastContactExit).toMatch(
      /animation-duration:\s*var\(--motion-fast\)/,
    );
    expect(contactsRowsDuringSwitch).toMatch(/animation:\s*none/);
    expect(contactsFallback).toMatch(/grid-column:\s*auto/);
    expect(contactsFallback).toMatch(/height:\s*100%/);
  });

  it("disables retraction in defensive layouts and removes staged motion when requested", () => {
    const compact = nestedBlockFor("@media (max-width: 940px)");
    const singlePane = nestedBlockFor("@media (max-width: 720px)");
    const reducedMotion = nestedBlockFor(
      "@media (prefers-reduced-motion: reduce)",
    );

    expect(compact).toMatch(
      /\.mail-workspace,[\s\S]*\.mail-workspace\[data-list-motion\][\s\S]*transition:\s*none/,
    );
    expect(singlePane).toMatch(
      /\.app-shell\.has-selection \.mail-list-motion-frame[\s\S]*display:\s*none/,
    );
    expect(reducedMotion).toMatch(
      /\.mail-list-motion-frame,[\s\S]*\.reader-panel--message,[\s\S]*\.contacts-detail-panel--selected[\s\S]*animation:\s*none !important/,
    );
    expect(reducedMotion).toMatch(
      /\.mail-workspace[\s\S]*transition:\s*none !important/,
    );
    expect(reducedMotion).toMatch(
      /\*,\s*\*::before,\s*\*::after\s*\{[\s\S]*transition-duration:\s*1ms !important/,
    );
  });
});

describe("mail synchronization feedback material contract", () => {
  it("forms a full-width structural band instead of another floating card", () => {
    const root = declarationsFor(":root");
    const feedback = declarationsFor("\\.mail-sync-feedback");
    const feedbackIcon = declarationsFor("\\.mail-sync-feedback svg");
    const entrance = nestedBlockFor("@keyframes mail-sync-feedback-in");

    expect(styles.match(/--sync-feedback-row-surface:/g)).toHaveLength(1);
    expect(styles.match(/--sync-feedback-surface:/g)).toHaveLength(1);
    expect(root).toMatch(
      /--sync-feedback-row-surface:\s*color-mix\(\s*in srgb,\s*var\(--mail-list-surface\)\s*44%,\s*var\(--row-hover-surface\)\s*\)/,
    );
    expect(root).toMatch(
      /--sync-feedback-surface:\s*color-mix\(\s*in srgb,\s*var\(--sync-feedback-row-surface\)\s*94%,\s*var\(--color-text\)\s*\)/,
    );
    expect(feedback).toMatch(/width:\s*calc\(100%\s*\+\s*2px\)/);
    expect(feedback).toMatch(/margin:\s*0\s+-1px/);
    expect(feedback).toMatch(/display:\s*grid/);
    expect(feedback).toMatch(
      /grid-template-columns:\s*34px\s+minmax\(0,\s*1fr\)\s+40px/,
    );
    expect(feedback).toMatch(/column-gap:\s*9px/);
    expect(feedback).toMatch(/padding:\s*5px\s+16px\s+5px\s+19px/);
    expect(feedback).toMatch(/border:\s*0/);
    expect(feedback).toMatch(/border-radius:\s*0/);
    expect(feedback).toMatch(/box-shadow:\s*none/);
    expect(feedback).toMatch(
      /background:\s*var\(--sync-feedback-surface\)/,
    );
    expect(feedback).toMatch(/min-height:\s*26px/);
    expect(feedback).toMatch(/font-weight:\s*400/);
    expect(feedback).toMatch(/color:\s*var\(--sync-feedback-text\)/);
    expect(root).toMatch(
      /--sync-feedback-text:\s*var\(--color-text-secondary\)/,
    );
    expect(feedbackIcon).toMatch(/justify-self:\s*start/);
    expect(feedbackIcon).toMatch(/margin-inline-start:\s*9px/);
    expect(entrance).not.toMatch(/transform/);
  });
});

describe("contact correspondence history material contract", () => {
  it("uses one continuous list and theme-colored direction glyphs without icon tiles", () => {
    const list = contactDeclarationsFor("\\.contacts-message-list");
    const row = contactDeclarationsFor("\\.contacts-message-row");
    const direction = contactDeclarationsFor(
      "\\.contacts-message-row__direction",
    );
    const outgoing = contactDeclarationsFor(
      '\\.contacts-message-row__direction\\[data-outgoing="true"\\]',
    );

    expect(list).toMatch(/gap:\s*0/);
    expect(list).toMatch(/border:\s*0/);
    expect(list).toMatch(/background:\s*transparent/);
    expect(row).toMatch(/border:\s*0/);
    expect(row).toMatch(/background:\s*transparent/);
    expect(direction).toMatch(/background:\s*transparent/);
    expect(direction).toMatch(/border:\s*0/);
    expect(direction).toMatch(/box-shadow:\s*none/);
    expect(direction).toMatch(/color:\s*var\(--color-primary\)/);
    expect(outgoing).toMatch(/color:\s*var\(--color-primary\)/);
    expect(direction).not.toMatch(/var\(--color-success\)/);
  });
});

describe("mail unread indicator contract", () => {
  it("clips long sender text without clipping the unread dot halo", () => {
    const sender = declarationsFor("\\.mail-row__sender");
    const senderText = declarationsFor("\\.mail-row__sender-text");

    expect(sender).not.toMatch(/overflow:\s*hidden/);
    expect(sender).toMatch(/min-width:\s*0/);
    expect(senderText).toMatch(/min-width:\s*0/);
    expect(senderText).toMatch(/overflow:\s*hidden/);
    expect(senderText).toMatch(/text-overflow:\s*ellipsis/);
    expect(senderText).toMatch(/white-space:\s*nowrap/);
  });
});

describe("sidebar delivery indicators", () => {
  it("uses motion for active Outbox work and a non-numeric dot for new Sent mail", () => {
    const outbox = declarationsFor("\\.folder-nav__activity--outbox svg");
    const sent = declarationsFor("\\.folder-nav__new-dot");

    expect(outbox).toMatch(/animation:\s*spin 900ms linear infinite/);
    expect(sent).toMatch(/width:\s*7px/);
    expect(sent).toMatch(/height:\s*7px/);
    expect(sent).toMatch(/border-radius:\s*50%/);
    expect(sent).not.toMatch(/font-size|font-variant-numeric/);
  });
});

describe("brand avatar sizing policy", () => {
  it("scales the sidebar brand lockup without wrapping its wordmark", () => {
    expect(declarationsFor("\\.sidebar__content")).toMatch(
      /container-type:\s*inline-size/,
    );

    const brand = declarationsFor("\\.brand");
    expect(brand).toMatch(
      /--sidebar-brand-logo-size:\s*clamp\(48px,\s*23cqi,\s*60px\)/,
    );
    expect(brand).toMatch(
      /--sidebar-brand-name-size:\s*clamp\(26px,\s*12\.5cqi,\s*35px\)/,
    );
    expect(brand).toMatch(/max-width:\s*100%/);
    expect(brand).toMatch(/min-width:\s*0/);

    const brandName = declarationsFor("\\.brand__name");
    expect(brandName).toMatch(/min-width:\s*0/);
    expect(brandName).toMatch(/overflow:\s*hidden/);
    expect(brandName).toMatch(/white-space:\s*nowrap/);
  });

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

describe("vertical scroll boundary policy", () => {
  it("stops immediately at the top and keeps the sidebar buffer at the bottom", () => {
    expect(declarationsFor("\\.vertical-scroll-surface")).toMatch(
      /overscroll-behavior-y:\s*none/,
    );

    const sidebarPrimary = declarationsFor("\\.sidebar__primary");
    expect(sidebarPrimary).toMatch(/margin-top:\s*24px/);
    expect(sidebarPrimary).toMatch(/padding-bottom:\s*14px/);
    expect(sidebarPrimary).toMatch(/scroll-padding-bottom:\s*14px/);
    expect(declarationsFor("\\.folder-nav")).toMatch(/margin-top:\s*0/);
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

describe("storage composition styling", () => {
  it("keeps the storage location in one labeled capsule row", () => {
    const location = declarationsFor("\\.settings-storage-location");
    expect(location).toMatch(
      /grid-template-columns:\s*max-content minmax\(0,\s*1fr\)/,
    );

    const capsule = declarationsFor("\\.settings-storage-location__capsule");
    expect(capsule).toMatch(/display:\s*flex/);
    expect(capsule).toMatch(/border-radius:\s*99px/);

    const changeButton = declarationsFor("\\.settings-storage-change");
    expect(changeButton).toMatch(/width:\s*36px/);
    expect(changeButton).toMatch(/height:\s*36px/);
    expect(changeButton).toMatch(/border-radius:\s*50%/);
  });

  it("uses one segmented track with hoverable category segments", () => {
    const root = declarationsFor("(?:^|\\r?\\n):root");
    [
      "mail",
      "webview",
      "user-assets",
      "cache",
      "logs",
      "other",
    ].forEach((category) => {
      expect(root).toContain(`--storage-category-${category}:`);
    });

    const composition = declarationsFor("\\.settings-storage-composition");
    expect(composition).toMatch(/display:\s*flex/);
    expect(composition).toMatch(/height:\s*14px/);
    expect(composition).toMatch(/overflow:\s*hidden/);

    const segment = declarationsFor(
      "\\.settings-storage-composition__segment",
    );
    expect(segment).toMatch(/cursor:\s*help/);
    expect(styles).not.toContain(".settings-storage-usage__caption");
    expect(styles).not.toContain(".settings-storage-legend");
    expect(styles).not.toContain(".settings-storage-usage__row");
    expect(styles).not.toContain("settings-storage-usage__row progress");
  });
});

describe("compose page and stationery policy", () => {
  it("keeps the solid compose page and the original rounded editor surface", () => {
    const panel = declarationsFor("(?:^|\\r?\\n)\\.compose-panel");
    expect(panel).toMatch(/background:\s*var\(--compose-page-surface\)/);
    expect(panel).not.toMatch(/backdrop-filter/);
    expect(panel).toMatch(/width var\(--motion-window\)/);
    expect(panel).toMatch(/height var\(--motion-window\)/);

    const minimizedHover = declarationsFor(
      "\\.compose-minimized-shell:hover,\\s*\\r?\\n\\.compose-minimized-shell:focus-within",
    );
    expect(minimizedHover).toMatch(/background:\s*color-mix/);

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

    const gridSpaceCaretActive = declarationsFor(
      '\\.compose-editor-shell\\[data-stationery="grid"\\]\\s*\\.compose-grid-space-caret-active',
    );
    expect(gridSpaceCaretActive).toMatch(/caret-color:\s*transparent/);

    const gridSpaceCaret = declarationsFor(
      '\\.compose-editor-shell\\[data-stationery="grid"\\]\\s*\\.compose-rich-editor\\s*\\.ProseMirror:focus\\s*\\.compose-grid-space-caret',
    );
    expect(gridSpaceCaret).toMatch(/display:\s*inline-block/);
    expect(gridSpaceCaret).toMatch(/width:\s*0/);
    expect(gridSpaceCaret).toMatch(
      /height:\s*var\(--compose-paper-cell-size\)/,
    );
    expect(gridSpaceCaret).toMatch(/pointer-events:\s*none/);

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

  it("uses a bounded loading buffer without persistent folder state chrome", () => {
    const sentinel = declarationsFor("\\.mail-pagination-sentinel");
    const loading = declarationsFor(
      '\\.mail-pagination-sentinel\\[data-state="loading"\\]',
    );
    const loadingIcon = declarationsFor(
      '\\.mail-pagination-sentinel\\[data-state="loading"\\] svg',
    );
    expect(sentinel).toMatch(/height:\s*1px/);
    expect(sentinel).toMatch(/display:\s*flex/);
    expect(sentinel).toMatch(/overflow:\s*hidden/);
    expect(sentinel).toMatch(/color:\s*var\(--color-text-muted\)/);
    expect(sentinel).toMatch(/height var\(--motion-normal\)/);
    expect(loading).toMatch(/height:\s*64px/);
    expect(loading).toMatch(/opacity:\s*1/);
    expect(loading).not.toMatch(/background|border|box-shadow/);
    expect(loadingIcon).toMatch(/color:\s*var\(--color-primary\)/);
    expect(loadingIcon).toMatch(/animation:\s*spin 850ms linear infinite/);
    expect(styles).not.toMatch(/mail-pagination-notice/);
    expect(styles).not.toMatch(/mail-load-progress|empty-list/);
    expect(styles).not.toMatch(
      /folder-nav__capability|data-capability-status|data-pending-count/,
    );
  });

  it("paints refreshed mail rows immediately when loading finishes", () => {
    const mailRow = declarationsFor("\\.mail-row");
    expect(mailRow).not.toMatch(/animation(?:-delay)?:/);
    const workspaceStyles = `${styles}\n${contactsStyles}`;
    expect(workspaceStyles).not.toMatch(/@keyframes\s+row-in/);
    expect(workspaceStyles).not.toContain("--row-index");
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
    const recipientToggle = declarationsFor("\\.recipient-toggle");
    expect(recipientToggle).toMatch(/padding:\s*0/);
    expect(recipientToggle).toMatch(/border:\s*0/);
    expect(recipientToggle).toMatch(/background:\s*transparent/);
    expect(recipientToggle).not.toMatch(/min-height/);
    const recipientDisclosure = declarationsFor(
      "\\.sender-card__recipient-disclosure",
    );
    expect(recipientDisclosure).toMatch(/position:\s*relative/);
    expect(recipientDisclosure).toMatch(/align-items:\s*center/);
    expect(recipientDisclosure).toMatch(/transform:\s*translateY\(12px\)/);
    const recipientDetails = declarationsFor(
      "\\.sender-card__recipient-disclosure > \\.recipient-details",
    );
    expect(recipientDetails).toMatch(/position:\s*absolute/);
    expect(recipientDetails).toMatch(/top:\s*50%/);
    expect(recipientDetails).toMatch(/transform:\s*translateY\(-50%\)/);
    expect(recipientDetails).toMatch(/border:\s*0/);
    expect(recipientDetails).toMatch(/background:\s*transparent/);
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
    expect(declarationsFor("\\.settings-help__button")).toMatch(
      /border:\s*0[\s\S]*background:\s*transparent/,
    );
    expect(declarationsFor("\\.settings-help__button::before")).toMatch(
      /width:\s*19px[\s\S]*height:\s*19px/,
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
      /transform:\s*translate3d\([\s\S]*--sliding-selection-x[\s\S]*--sliding-selection-y/,
    );
    expect(declarationsFor("\\.folder-nav__selection")).toMatch(
      /transform var\(--motion-normal\)/,
    );
    expect(declarationsFor("\\.folder-nav__selection")).toMatch(
      /opacity:\s*0[\s\S]*scale\(0\.97\)/,
    );
    expect(declarationsFor("\\.folder-nav__selection")).toMatch(
      /opacity var\(--motion-fast\) ease-out/,
    );
    expect(
      declarationsFor(
        '\\.folder-nav__selection\\[data-visible="true"\\]',
      ),
    ).toMatch(/opacity:\s*1[\s\S]*scale\(1\)/);
    expect(declarationsFor("\\.folder-nav__item,\\s*\\.sidebar-action")).toMatch(
      /font-weight var\(--motion-fast\)/,
    );
    expect(
      declarationsFor('\\.folder-nav__item\\[data-selected="true"\\]'),
    ).toMatch(/background:\s*transparent/);
  });

  it("moves one theme-owned mail selection surface without moving rows", () => {
    expect(declarationsFor("\\.mail-list")).toMatch(
      /position:\s*relative[\s\S]*isolation:\s*isolate/,
    );
    expect(declarationsFor("\\.mail-list::before")).toMatch(
      /background:\s*var\(--row-selection-surface\)/,
    );
    expect(declarationsFor("\\.mail-list::before")).toMatch(
      /transform:\s*translate3d\([\s\S]*--sliding-selection-x[\s\S]*--sliding-selection-y/,
    );
    expect(declarationsFor("\\.mail-list::before")).toMatch(
      /transform var\(--motion-normal\)/,
    );
    expect(
      declarationsFor('\\.mail-row\\[data-selected="true"\\]'),
    ).toMatch(/background:\s*transparent/);
  });

  it("insets an adjacent mail hover surface without moving rows", () => {
    expect(declarationsFor("\\.mail-row::before")).toMatch(
      /inset:\s*2px 0[\s\S]*background:\s*transparent/,
    );
    expect(declarationsFor("\\.mail-row:hover::before")).toMatch(
      /background:\s*var\(--row-hover-surface\)/,
    );
    expect(declarationsFor("\\.mail-row:hover")).toMatch(
      /background:\s*transparent/,
    );
    expect(
      declarationsFor('\\.mail-row\\[data-selected="true"\\]::before'),
    ).toMatch(
      /background:\s*transparent/,
    );
  });

  it("moves theme-owned account and settings selection surfaces", () => {
    expect(declarationsFor("\\.account-switcher")).toMatch(
      /position:\s*relative[\s\S]*isolation:\s*isolate/,
    );
    expect(declarationsFor("\\.account-switcher::before")).toMatch(
      /background:\s*var\(--account-card-active-surface\)/,
    );
    expect(declarationsFor("\\.account-switcher::before")).toMatch(
      /translate var\(--motion-normal\)/,
    );
    expect(declarationsFor("\\.account-switcher::before")).toMatch(
      /transform var\(--account-selection-transform-duration\)/,
    );
    expect(
      declarationsFor(
        '\\.account-switcher:has\\(\\.account-card\\[data-active="true"\\]:hover\\)',
      ),
    ).toMatch(
      /--account-selection-lift:\s*-1px[\s\S]*--account-selection-scale:\s*1\.006/,
    );
    expect(
      declarationsFor('\\.account-card\\[data-active="true"\\]'),
    ).toMatch(/background:\s*transparent/);

    expect(declarationsFor("\\.settings-nav")).toMatch(
      /position:\s*relative[\s\S]*isolation:\s*isolate/,
    );
    expect(declarationsFor("\\.settings-nav::before")).toMatch(
      /var\(--color-primary\) 12%[\s\S]*var\(--compose-control-surface\)/,
    );
    expect(declarationsFor("\\.settings-nav::before")).toMatch(
      /transform var\(--motion-normal\)/,
    );
    expect(
      declarationsFor('\\.settings-nav button\\[data-selected="true"\\]'),
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
    expect(reducedMotion).toMatch(
      /\.mail-list::before[\s\S]*will-change:\s*auto/,
    );
    expect(reducedMotion).toMatch(
      /\.account-switcher::before,[\s\S]*\.settings-nav::before[\s\S]*will-change:\s*auto/,
    );
    expect(forcedColors).toMatch(
      /\.folder-nav__selection[\s\S]*background:\s*Highlight/,
    );
    expect(forcedColors).toMatch(
      /\.mail-list::before[\s\S]*background:\s*Highlight/,
    );
    expect(forcedColors).toMatch(
      /\.account-switcher::before,[\s\S]*\.settings-nav::before[\s\S]*background:\s*Highlight/,
    );
    expect(forcedColors).toMatch(
      /\.compose-forward-context\[data-immutable="true"\][\s\S]*forced-color-adjust:\s*auto/,
    );
  });
});
