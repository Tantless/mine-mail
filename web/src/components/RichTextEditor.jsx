import {
  Eraser,
  LinkSimple,
  ListBullets,
  ListNumbers,
  TextAlignCenter,
  TextAlignLeft,
  TextAlignRight,
  TextB,
  TextItalic,
  TextUnderline,
  X,
} from "@phosphor-icons/react";
import { Extension, InputRule } from "@tiptap/core";
import { EditorContent, useEditor, useEditorState } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { BulletList, OrderedList } from "@tiptap/extension-list";
import { TextAlign } from "@tiptap/extension-text-align";
import { TextStyleKit } from "@tiptap/extension-text-style";
import { Plugin } from "@tiptap/pm/state";
import { canJoin, findWrapping } from "@tiptap/pm/transform";
import { Decoration, DecorationSet } from "@tiptap/pm/view";
import {
  Component,
  Fragment,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { IconButton } from "./IconButton.jsx";
import { ThemedSelect } from "./ThemedSelect.jsx";
import "./RichTextEditor.css";

const blockTags = new Set(["BLOCKQUOTE", "DIV", "LI", "P"]);
const composeBaseFontSize = 14;
const allowedTags = new Set([
  "A",
  "B",
  "BLOCKQUOTE",
  "BR",
  "DIV",
  "EM",
  "FONT",
  "I",
  "LI",
  "OL",
  "P",
  "SPAN",
  "STRONG",
  "U",
  "UL",
]);
const alignments = new Set(["left", "center", "right"]);
const fontOptions = [
  { value: "system", label: "系统字体" },
  { value: "Microsoft YaHei", label: "微软雅黑" },
  { value: "SimSun", label: "宋体" },
  { value: "KaiTi", label: "楷体" },
  { value: "Consolas", label: "等宽字体" },
];
const fontFamilies = new Map([
  ["arial", "Arial"],
  ["sans-serif", "Arial"],
  ["microsoft yahei", "Microsoft YaHei"],
  ["simsun", "SimSun"],
  ["kaiti", "KaiTi"],
  ["consolas", "Consolas"],
  ["serif", "serif"],
  ["monospace", "monospace"],
]);
const sizeOptions = [12, 14, 16, 18, 22].map((size) => ({
  value: String(size),
  label: String(size),
}));
const legacySizeByPixels = new Map([
  ["12", "2"],
  ["14", "3"],
  ["16", "4"],
  ["18", "5"],
  ["22", "6"],
]);
const pixelsByLegacySize = new Map(
  [...legacySizeByPixels].map(([pixels, legacy]) => [legacy, pixels]),
);
const composePaperInlineInset = 12;
const composePaperBlockInset = 10;
const latinGridCharacter = /[\p{Script=Latin}\p{Number}]/u;
const hanGridCharacter = /\p{Script=Han}/u;
const gridWhitespaceCharacter = /\s/u;
const paragraphIndentAttribute = "data-first-line-indent";
const paragraphIndentValue = "tab";
const paragraphIndentStyle = "2em";
const legacyParagraphIndentStyle = "4em";

export function groupGridTextTokens(text) {
  const tokens = [];
  const characters = Array.from(String(text || ""));
  let offset = 0;
  let index = 0;

  while (index < characters.length) {
    const character = characters[index];
    const start = offset;

    if (latinGridCharacter.test(character)) {
      const group = [];
      while (
        index < characters.length &&
        group.length < 3 &&
        latinGridCharacter.test(characters[index])
      ) {
        group.push(characters[index]);
        offset += characters[index].length;
        index += 1;
      }
      tokens.push({
        from: start,
        to: offset,
        kind: "latin",
        text: group.join(""),
      });
      continue;
    }

    offset += character.length;
    index += 1;
    tokens.push({
      from: start,
      to: offset,
      kind: hanGridCharacter.test(character)
        ? "han"
        : gridWhitespaceCharacter.test(character)
          ? "space"
          : "special",
      text: character,
    });
  }

  return tokens;
}

function editorIsUsable(editor) {
  if (!editor || editor.isDestroyed) return false;
  try {
    return Boolean(editor.state?.schema && editor.view?.dom);
  } catch {
    return false;
  }
}

function markPreservingListRule({
  find,
  type,
  getAttributes,
  getActiveMarks,
  joinPredicate,
  setPendingMarks,
}) {
  return new InputRule({
    find,
    handler: ({ state, range, match }) => {
      const editorMarks = getActiveMarks?.() || null;
      const inheritedMarks = state.selection.$from.marks();
      const activeMarks =
        editorMarks?.length
          ? editorMarks
          : state.storedMarks?.length
          ? state.storedMarks
          : inheritedMarks.length
            ? inheritedMarks
            : state.selection.$from.nodeBefore?.marks || null;
      const attributes = getAttributes?.(match) || {};
      const transaction = state.tr.delete(range.from, range.to);
      const start = transaction.doc.resolve(range.from);
      const blockRange = start.blockRange();
      const wrapping = blockRange && findWrapping(blockRange, type, attributes);
      if (!wrapping) return null;

      transaction.wrap(blockRange, wrapping);

      const before = transaction.doc.resolve(range.from - 1).nodeBefore;
      if (
        before &&
        before.type === type &&
        canJoin(transaction.doc, range.from - 1) &&
        (!joinPredicate || joinPredicate(match, before))
      ) {
        transaction.join(range.from - 1);
      }
      if (activeMarks?.length) {
        setPendingMarks?.(activeMarks, transaction.selection.from);
      }
      return undefined;
    },
  });
}

const ComposeListInputRules = Extension.create({
  name: "composeListInputRules",
  priority: 1100,
  addStorage() {
    return {
      pendingMarks: null,
    };
  },
  addInputRules() {
    const orderedList = this.editor.schema.nodes.orderedList;
    const bulletList = this.editor.schema.nodes.bulletList;
    const getActiveMarks = () => {
      const { selection, storedMarks } = this.editor.state;
      return storedMarks?.length
        ? storedMarks
        : selection.$from.nodeBefore?.marks || selection.$from.marks();
    };
    const setPendingMarks = (marks, position) => {
      this.storage.pendingMarks = { marks, position };
    };
    return [
      markPreservingListRule({
        find: /^(\d+)\.\s$/,
        type: orderedList,
        getActiveMarks,
        setPendingMarks,
        getAttributes: (match) => ({ start: Number(match[1]) || 1 }),
        joinPredicate: (match, node) =>
          node.childCount + Number(node.attrs.start || 1) === Number(match[1]),
      }),
      markPreservingListRule({
        find: /^\s*([-+*])\s$/,
        type: bulletList,
        getActiveMarks,
        setPendingMarks,
      }),
    ];
  },
  addProseMirrorPlugins() {
    return [
      new Plugin({
        props: {
          decorations: (state) => {
            if (!state.selection.empty) return null;
            const pending = this.storage.pendingMarks;
            const marks =
              state.storedMarks?.length
                ? state.storedMarks
                : pending?.position === state.selection.from
                  ? pending.marks
                  : null;
            if (!marks?.length) return null;
            const marker = document.createElement("img");
            marker.className = "compose-format-caret-probe";
            marker.setAttribute("alt", "");
            marker.setAttribute("aria-hidden", "true");
            return DecorationSet.create(state.doc, [
              Decoration.widget(state.selection.from, marker, {
                key: "compose-format-caret-probe",
                marks,
                raw: true,
                side: -1,
              }),
            ]);
          },
          handleTextInput: (view, _from, _to, text) => {
            const pending = this.storage.pendingMarks;
            if (
              !pending?.marks?.length ||
              pending.position !== view.state.selection.from
            ) {
              return false;
            }
            this.storage.pendingMarks = null;
            const textNode = view.state.schema.text(text, pending.marks);
            view.dispatch(
              view.state.tr
                .replaceSelectionWith(textNode, false)
                .scrollIntoView(),
            );
            return true;
          },
        },
      }),
    ];
  },
});

const ComposeBulletList = BulletList.extend({
  addInputRules() {
    return [];
  },
});

const ComposeOrderedList = OrderedList.extend({
  addInputRules() {
    return [];
  },
});

function activeParagraphContext(editor) {
  const { selection } = editor.state;
  if (!selection.empty || selection.$from.parent.type.name !== "paragraph") {
    return null;
  }
  const { $from } = selection;
  const insideList = Array.from({ length: $from.depth }, (_, index) =>
    $from.node(index + 1),
  ).some((node) => node.type.name === "listItem");
  return {
    firstLineIndent: $from.parent.attrs.firstLineIndent,
    insideList,
    parentOffset: $from.parentOffset,
  };
}

const ComposeParagraphIndent = Extension.create({
  name: "composeParagraphIndent",
  priority: 1050,
  addGlobalAttributes() {
    return [
      {
        types: ["paragraph"],
        attributes: {
          firstLineIndent: {
            default: null,
            parseHTML: (element) => {
              const explicitValue = element.getAttribute(
                paragraphIndentAttribute,
              );
              const textIndent = String(element.style?.textIndent || "")
                .replace(/\s+/g, "")
                .toLowerCase();
              return explicitValue === paragraphIndentValue ||
                textIndent === paragraphIndentStyle ||
                textIndent === legacyParagraphIndentStyle
                ? paragraphIndentValue
                : null;
            },
            renderHTML: (attributes) =>
              attributes.firstLineIndent === paragraphIndentValue
                ? {
                    [paragraphIndentAttribute]: paragraphIndentValue,
                    style: `text-indent: ${paragraphIndentStyle};`,
                  }
                : {},
          },
        },
      },
    ];
  },
  addKeyboardShortcuts() {
    const setIndent = (firstLineIndent) => {
      const context = activeParagraphContext(this.editor);
      if (!context || context.insideList || context.parentOffset !== 0) {
        return false;
      }
      return this.editor.commands.updateAttributes("paragraph", {
        firstLineIndent,
      });
    };
    return {
      Tab: () => setIndent(paragraphIndentValue),
      "Shift-Tab": () => setIndent(null),
      Enter: () => {
        const context = activeParagraphContext(this.editor);
        if (
          !context ||
          context.insideList ||
          context.firstLineIndent !== paragraphIndentValue
        ) {
          return false;
        }
        return this.editor
          .chain()
          .splitBlock()
          .updateAttributes("paragraph", {
            firstLineIndent: paragraphIndentValue,
          })
          .run();
      },
    };
  },
});

const ComposeGridCellTokens = Extension.create({
  name: "composeGridCellTokens",
  addProseMirrorPlugins() {
    return [
      new Plugin({
        props: {
          decorations: (state) => {
            const decorations = [];
            state.doc.descendants((node, position) => {
              if (!node.isText || !node.text) return;
              groupGridTextTokens(node.text).forEach((token) => {
                decorations.push(
                  Decoration.inline(
                    position + token.from,
                    position + token.to,
                    {
                      class: "compose-grid-cell-token",
                      "data-grid-token-kind": token.kind,
                    },
                    {
                      inclusiveStart: false,
                      inclusiveEnd: false,
                    },
                  ),
                );
              });
            });
            return decorations.length
              ? DecorationSet.create(state.doc, decorations)
              : null;
          },
        },
      }),
    ];
  },
});

export const composeInputRuleExtensions = ["composeListInputRules"];

export function createComposeEditorExtensions() {
  return [
    StarterKit.configure({
      blockquote: false,
      code: false,
      codeBlock: false,
      heading: false,
      horizontalRule: false,
      strike: false,
      bulletList: false,
      orderedList: false,
      link: {
        autolink: false,
        linkOnPaste: false,
        openOnClick: false,
      },
    }),
    TextStyleKit.configure({
      backgroundColor: false,
      color: false,
      lineHeight: false,
    }),
    TextAlign.configure({
      types: ["paragraph"],
      alignments: ["left", "center", "right"],
    }),
    ComposeBulletList.configure({
      keepMarks: true,
      keepAttributes: true,
    }),
    ComposeOrderedList.configure({
      keepMarks: true,
      keepAttributes: true,
    }),
    ComposeListInputRules,
    ComposeParagraphIndent,
    ComposeGridCellTokens,
  ];
}

function safeHref(value) {
  const href = value?.trim();
  if (!href) return null;
  try {
    const parsed = new URL(href, window.location.origin);
    return ["http:", "https:", "mailto:"].includes(parsed.protocol)
      ? href
      : null;
  } catch {
    return null;
  }
}

function safeFontFamily(value) {
  const normalized = String(value || "")
    .trim()
    .replace(/^["']|["']$/g, "")
    .toLowerCase();
  return fontFamilies.get(normalized) || null;
}

function safeFontSize(value) {
  const match = String(value || "")
    .trim()
    .match(/^(\d+(?:\.\d+)?)px$/i);
  if (!match) return null;
  const rounded = String(Math.round(Number(match[1])));
  return legacySizeByPixels.has(rounded) ? rounded : null;
}

function appendSanitizedNode(source, target) {
  if (source.nodeType === Node.TEXT_NODE) {
    target.append(document.createTextNode(source.textContent || ""));
    return;
  }
  if (source.nodeType !== Node.ELEMENT_NODE) return;

  const tagName = source.tagName.toUpperCase();
  if (tagName === "SCRIPT" || tagName === "STYLE") return;
  if (!allowedTags.has(tagName)) {
    [...source.childNodes].forEach((child) => appendSanitizedNode(child, target));
    return;
  }

  const spanFontFamily =
    tagName === "SPAN" ? safeFontFamily(source.style?.fontFamily) : null;
  const spanFontSize =
    tagName === "SPAN" ? safeFontSize(source.style?.fontSize) : null;
  const outputTagName =
    tagName === "B"
      ? "STRONG"
      : tagName === "SPAN" && (spanFontFamily || spanFontSize)
        ? "FONT"
        : tagName;
  const element = document.createElement(outputTagName.toLowerCase());

  if (tagName === "A") {
    const href = safeHref(source.getAttribute("href"));
    if (href) {
      element.setAttribute("href", href);
      element.setAttribute("rel", "noopener noreferrer");
    }
  }

  if (tagName === "FONT") {
    const face = safeFontFamily(source.getAttribute("face"));
    const size = source.getAttribute("size")?.trim();
    if (face) element.setAttribute("face", face);
    if (size && pixelsByLegacySize.has(size)) element.setAttribute("size", size);
  } else if (outputTagName === "FONT") {
    if (spanFontFamily) element.setAttribute("face", spanFontFamily);
    if (spanFontSize) {
      element.setAttribute("size", legacySizeByPixels.get(spanFontSize));
    }
  }

  if (tagName === "OL") {
    const start = source.getAttribute("start")?.trim();
    if (start && /^\d{1,4}$/.test(start) && Number(start) > 0) {
      element.setAttribute("start", start);
    }
  }
  if (tagName === "LI") {
    const value = source.getAttribute("value")?.trim();
    if (value && /^\d{1,4}$/.test(value) && Number(value) > 0) {
      element.setAttribute("value", value);
    }
  }

  const alignment = (
    source.getAttribute("align") ||
    source.style?.textAlign ||
    ""
  ).toLowerCase();
  if (blockTags.has(tagName) && alignments.has(alignment)) {
    element.setAttribute("align", alignment);
  }
  const textIndent = String(source.style?.textIndent || "")
    .replace(/\s+/g, "")
    .toLowerCase();
  if (
    (tagName === "P" || tagName === "DIV") &&
    (source.getAttribute(paragraphIndentAttribute) === paragraphIndentValue ||
      textIndent === paragraphIndentStyle ||
      textIndent === legacyParagraphIndentStyle)
  ) {
    element.setAttribute(paragraphIndentAttribute, paragraphIndentValue);
    element.style.textIndent = paragraphIndentStyle;
  }

  [...source.childNodes].forEach((child) => appendSanitizedNode(child, element));
  target.append(element);
}

export function normalizeComposeHtml(source) {
  if (!source?.trim()) return "";
  const template = document.createElement("template");
  template.innerHTML = source;
  const output = document.createElement("div");
  [...template.content.childNodes].forEach((node) =>
    appendSanitizedNode(node, output),
  );
  const html = output.innerHTML.trim();
  const visibleText = (output.textContent || "")
    .replace(/\u00a0/g, " ")
    .trim();
  return visibleText ? html : "";
}

export function composeHtmlToEditorHtml(source) {
  const normalized = normalizeComposeHtml(source);
  if (!normalized) return "";
  const template = document.createElement("template");
  template.innerHTML = normalized;

  [...template.content.querySelectorAll("font")].forEach((font) => {
    const span = document.createElement("span");
    const face = safeFontFamily(font.getAttribute("face"));
    const pixels = pixelsByLegacySize.get(font.getAttribute("size") || "");
    if (face) span.style.fontFamily = face;
    if (pixels) span.style.fontSize = `${pixels}px`;
    span.append(...font.childNodes);
    font.replaceWith(span);
  });

  [...template.content.querySelectorAll("[align]")].forEach((block) => {
    const alignment = block.getAttribute("align")?.toLowerCase();
    if (alignments.has(alignment)) block.style.textAlign = alignment;
    block.removeAttribute("align");
  });

  [...template.content.querySelectorAll("div")].forEach((block) => {
    const paragraph = document.createElement("p");
    [...block.attributes].forEach((attribute) =>
      paragraph.setAttribute(attribute.name, attribute.value),
    );
    paragraph.append(...block.childNodes);
    block.replaceWith(paragraph);
  });

  return template.innerHTML;
}

function textToHtml(text) {
  if (!text) return "";
  const output = document.createElement("div");
  String(text)
    .replace(/\r\n?/g, "\n")
    .split("\n")
    .forEach((line) => {
      const block = document.createElement("p");
      if (line) block.textContent = line;
      else block.append(document.createElement("br"));
      output.append(block);
    });
  return output.innerHTML;
}

function inlineText(node) {
  if (!node) return "";
  if (node.type === "text") return node.text || "";
  if (node.type === "hardBreak") return "\n";
  return (node.content || []).map(inlineText).join("");
}

function listText(node, depth = 0) {
  const ordered = node.type === "orderedList";
  const start = Number(node.attrs?.start) || 1;
  return (node.content || [])
    .map((item, index) => {
      const prefix = ordered ? `${start + index}. ` : "• ";
      const indent = "  ".repeat(depth);
      const paragraphs = [];
      const nested = [];
      (item.content || []).forEach((child) => {
        if (child.type === "orderedList" || child.type === "bulletList") {
          nested.push(listText(child, depth + 1));
        } else {
          paragraphs.push(inlineText(child));
        }
      });
      const ownText = paragraphs.join("\n");
      return `${indent}${prefix}${ownText}${
        nested.length ? `\n${nested.join("\n")}` : ""
      }`;
    })
    .join("\n");
}

export function plainTextFromDocument(documentJson) {
  return (documentJson?.content || [])
    .map((node) =>
      node.type === "orderedList" || node.type === "bulletList"
        ? listText(node)
        : inlineText(node),
    )
    .join("\n")
    .replace(/\u00a0/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .replace(/\n+$/g, "");
}

function textStyleValue(editor, attribute, fallback) {
  const { from, to, empty } = editor.state.selection;
  if (empty) {
    const pending = editor.storage.composeListInputRules?.pendingMarks;
    if (pending?.position === from) {
      const textStyle = pending.marks.find(
        (mark) => mark.type.name === "textStyle",
      );
      if (textStyle?.attrs?.[attribute]) return textStyle.attrs[attribute];
    }
    return editor.getAttributes("textStyle")[attribute] || fallback;
  }

  const values = new Set();
  editor.state.doc.nodesBetween(from, to, (node) => {
    if (!node.isText) return;
    const textStyle = node.marks.find((mark) => mark.type.name === "textStyle");
    values.add(textStyle?.attrs?.[attribute] || fallback);
  });
  return values.size === 1 ? [...values][0] : null;
}

export function getComposeToolbarState(editor) {
  if (!editorIsUsable(editor)) {
    return {
      font: "system",
      fontSize: String(composeBaseFontSize),
      mixedFont: false,
      mixedFontSize: false,
    };
  }

  const rawFont = textStyleValue(editor, "fontFamily", "Arial");
  const rawFontSize = textStyleValue(
    editor,
    "fontSize",
    `${composeBaseFontSize}px`,
  );
  const normalizedFont = rawFont ? safeFontFamily(rawFont) : null;
  const normalizedFontSize = rawFontSize ? safeFontSize(rawFontSize) : null;

  return {
    font:
      normalizedFont === "Arial" || !normalizedFont ? "system" : normalizedFont,
    fontSize: normalizedFontSize || String(composeBaseFontSize),
    mixedFont: rawFont === null,
    mixedFontSize: rawFontSize === null,
    bold: editor.isActive("bold"),
    italic: editor.isActive("italic"),
    underline: editor.isActive("underline"),
    bulletList: editor.isActive("bulletList"),
    orderedList: editor.isActive("orderedList"),
    alignment: editor.getAttributes("paragraph").textAlign || "left",
  };
}

function ToolbarButton({ label, active = false, children, onActivate, disabled }) {
  return (
    <IconButton
      className="compose-format-button"
      label={label}
      aria-pressed={active}
      disabled={disabled}
      onPointerDown={(event) => event.preventDefault()}
      onClick={onActivate}
    >
      {children}
    </IconButton>
  );
}

class RichTextEditorBoundary extends Component {
  state = {
    failed: false,
    retryKey: 0,
  };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  retry = () => {
    this.setState((current) => ({
      failed: false,
      retryKey: current.retryKey + 1,
    }));
  };

  render() {
    if (this.state.failed) {
      return (
        <div className="compose-editor-recovery" role="alert">
          <strong>编辑器暂时无法载入</strong>
          <span>正文内容仍保留在草稿中，请重试。</span>
          <button type="button" onClick={this.retry}>
            重新载入编辑器
          </button>
        </div>
      );
    }

    return (
      <Fragment key={this.state.retryKey}>{this.props.children}</Fragment>
    );
  }
}

function RichTextEditorCore({
  bodyText,
  format,
  stationery = "none",
  disabled = false,
  onChange,
  onEditorReady,
}) {
  const acceptEditorUpdatesRef = useRef(false);
  const pendingEmittedHtmlRef = useRef([]);
  const linkInputRef = useRef(null);
  const [paperCellSize, setPaperCellSize] = useState(composeBaseFontSize * 2);
  const [paperGridMinHeight, setPaperGridMinHeight] = useState(
    composeBaseFontSize * 8,
  );
  const [showLinkEditor, setShowLinkEditor] = useState(false);
  const [linkValue, setLinkValue] = useState("");
  const editorShellRef = useRef(null);

  const incomingHtml = useMemo(
    () => normalizeComposeHtml(format?.body_html || textToHtml(bodyText)),
    [bodyText, format?.body_html],
  );
  const incomingEditorHtml = useMemo(
    () => composeHtmlToEditorHtml(incomingHtml),
    [incomingHtml],
  );
  const latestFormatRef = useRef(format);
  const onChangeRef = useRef(onChange);
  const lastObservedHtmlRef = useRef(incomingHtml);
  latestFormatRef.current = format;
  onChangeRef.current = onChange;

  const editor = useEditor({
    extensions: createComposeEditorExtensions(),
    enableInputRules: composeInputRuleExtensions,
    content: incomingEditorHtml,
    editable: !disabled,
    editorProps: {
      attributes: {
        "aria-label": "邮件正文",
        "aria-multiline": "true",
        "aria-readonly": String(disabled),
        role: "textbox",
      },
      transformPastedHTML: (html) => composeHtmlToEditorHtml(html),
    },
    onUpdate: ({ editor: currentEditor }) => {
      if (
        !acceptEditorUpdatesRef.current ||
        !editorIsUsable(currentEditor)
      ) {
        return;
      }
      const html = normalizeComposeHtml(currentEditor.getHTML());
      if (html === lastObservedHtmlRef.current) return;
      lastObservedHtmlRef.current = html;
      pendingEmittedHtmlRef.current.push(html);
      onChangeRef.current?.({
        body_text: plainTextFromDocument(currentEditor.getJSON()),
        format: {
          ...latestFormatRef.current,
          body_html: html || null,
        },
      });
    },
  });

  const currentToolbarState = useEditorState({
    editor,
    selector: ({ editor: currentEditor }) =>
      getComposeToolbarState(currentEditor),
  });
  const currentFontOptions = useMemo(
    () =>
      currentToolbarState?.mixedFont
        ? [{ value: "mixed", label: "—", disabled: true }, ...fontOptions]
        : fontOptions,
    [currentToolbarState?.mixedFont],
  );
  const currentSizeOptions = useMemo(
    () =>
      currentToolbarState?.mixedFontSize
        ? [{ value: "mixed", label: "—", disabled: true }, ...sizeOptions]
        : sizeOptions,
    [currentToolbarState?.mixedFontSize],
  );

  useEffect(() => {
    if (!editorIsUsable(editor)) return;
    editor.setEditable(!disabled);
    editorShellRef.current
      ?.querySelector('[role="textbox"]')
      ?.setAttribute("aria-readonly", String(disabled));
  }, [disabled, editor]);

  useEffect(() => {
    if (!editorIsUsable(editor) || !onEditorReady) return undefined;
    onEditorReady(editor);
    return () => onEditorReady(null);
  }, [editor, onEditorReady]);

  useEffect(() => {
    if (!editorIsUsable(editor)) return;
    const pendingIndex = pendingEmittedHtmlRef.current.indexOf(incomingHtml);
    if (pendingIndex >= 0) {
      pendingEmittedHtmlRef.current.splice(0, pendingIndex + 1);
      acceptEditorUpdatesRef.current = true;
      return;
    }

    const currentHtml = normalizeComposeHtml(editor.getHTML());
    lastObservedHtmlRef.current = incomingHtml;
    if (currentHtml !== incomingHtml) {
      pendingEmittedHtmlRef.current = [];
      acceptEditorUpdatesRef.current = false;
      editor.commands.setContent(incomingEditorHtml, { emitUpdate: false });
    }
    acceptEditorUpdatesRef.current = true;
  }, [bodyText, editor, format, incomingEditorHtml, incomingHtml]);

  useEffect(() => {
    if (!showLinkEditor) return;
    window.requestAnimationFrame(() => linkInputRef.current?.focus());
  }, [showLinkEditor]);

  useLayoutEffect(() => {
    const editorElement = editorShellRef.current;
    if (!editorElement) return undefined;
    const targetCellSize = composeBaseFontSize * 2;
    const updatePaperMetrics = () => {
      const availableWidth = Math.max(
        0,
        editorElement.clientWidth - composePaperInlineInset * 2,
      );
      if (!availableWidth) {
        setPaperCellSize(targetCellSize);
        setPaperGridMinHeight(targetCellSize * 4);
        return;
      }
      const columnCount = Math.max(
        8,
        Math.floor(availableWidth / targetCellSize),
      );
      const nextCellSize = availableWidth / columnCount;
      const availableHeight = Math.max(
        nextCellSize * 4,
        editorElement.clientHeight - composePaperBlockInset * 2,
      );
      const rowCount = Math.max(
        4,
        Math.floor(availableHeight / nextCellSize),
      );
      const nextGridMinHeight = rowCount * nextCellSize;
      setPaperCellSize((current) =>
        Math.abs(current - nextCellSize) > 0.1 ? nextCellSize : current,
      );
      setPaperGridMinHeight((current) =>
        Math.abs(current - nextGridMinHeight) > 0.1
          ? nextGridMinHeight
          : current,
      );
    };

    updatePaperMetrics();
    window.addEventListener("resize", updatePaperMetrics);
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(updatePaperMetrics);
    observer?.observe(editorElement);

    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", updatePaperMetrics);
    };
  }, [editor]);

  const runEditorCommand = (command) => {
    if (!editorIsUsable(editor) || disabled) return;
    command(editor.chain().focus()).run();
  };

  const applyFont = (nextFont) => {
    if (nextFont === "mixed") return;
    runEditorCommand((chain) =>
      chain.setFontFamily(nextFont === "system" ? "Arial" : nextFont),
    );
  };

  const applySize = (nextSize) => {
    if (nextSize === "mixed") return;
    runEditorCommand((chain) => chain.setFontSize(`${nextSize}px`));
  };

  const insertLink = () => {
    const href = safeHref(linkValue);
    if (!href) return;
    runEditorCommand((chain) => chain.setLink({ href }));
    setLinkValue("");
    setShowLinkEditor(false);
  };

  const state = currentToolbarState || getComposeToolbarState(editor);
  return (
    <>
      <div className="compose-format-toolbar" role="toolbar" aria-label="正文格式">
        <ThemedSelect
          id="compose-font"
          className="compose-format-select compose-format-select--font"
          label="字体"
          value={state.mixedFont ? "mixed" : state.font}
          options={currentFontOptions}
          onValueChange={applyFont}
          disabled={disabled}
        />
        <ThemedSelect
          id="compose-font-size"
          className="compose-format-select compose-format-select--size"
          label="字号"
          value={state.mixedFontSize ? "mixed" : state.fontSize}
          options={currentSizeOptions}
          onValueChange={applySize}
          disabled={disabled}
        />
        <span className="compose-format-divider" aria-hidden="true" />
        <ToolbarButton
          label="粗体"
          active={state.bold}
          onActivate={() => runEditorCommand((chain) => chain.toggleBold())}
          disabled={disabled}
        >
          <TextB size={17} weight="bold" />
        </ToolbarButton>
        <ToolbarButton
          label="斜体"
          active={state.italic}
          onActivate={() => runEditorCommand((chain) => chain.toggleItalic())}
          disabled={disabled}
        >
          <TextItalic size={17} />
        </ToolbarButton>
        <ToolbarButton
          label="下划线"
          active={state.underline}
          onActivate={() => runEditorCommand((chain) => chain.toggleUnderline())}
          disabled={disabled}
        >
          <TextUnderline size={17} />
        </ToolbarButton>
        <span className="compose-format-divider" aria-hidden="true" />
        <ToolbarButton
          label="项目符号列表"
          active={state.bulletList}
          onActivate={() =>
            runEditorCommand((chain) => chain.toggleBulletList())
          }
          disabled={disabled}
        >
          <ListBullets size={18} />
        </ToolbarButton>
        <ToolbarButton
          label="编号列表"
          active={state.orderedList}
          onActivate={() =>
            runEditorCommand((chain) => chain.toggleOrderedList())
          }
          disabled={disabled}
        >
          <ListNumbers size={18} />
        </ToolbarButton>
        <span className="compose-format-divider" aria-hidden="true" />
        <ToolbarButton
          label="左对齐"
          active={state.alignment === "left"}
          onActivate={() =>
            runEditorCommand((chain) => chain.setTextAlign("left"))
          }
          disabled={disabled}
        >
          <TextAlignLeft size={18} />
        </ToolbarButton>
        <ToolbarButton
          label="居中对齐"
          active={state.alignment === "center"}
          onActivate={() =>
            runEditorCommand((chain) => chain.setTextAlign("center"))
          }
          disabled={disabled}
        >
          <TextAlignCenter size={18} />
        </ToolbarButton>
        <ToolbarButton
          label="右对齐"
          active={state.alignment === "right"}
          onActivate={() =>
            runEditorCommand((chain) => chain.setTextAlign("right"))
          }
          disabled={disabled}
        >
          <TextAlignRight size={18} />
        </ToolbarButton>
        <span className="compose-format-divider" aria-hidden="true" />
        <span className="compose-link-control">
          <ToolbarButton
            label="添加链接"
            active={showLinkEditor}
            onActivate={() => setShowLinkEditor((current) => !current)}
            disabled={disabled}
          >
            <LinkSimple size={17} />
          </ToolbarButton>
          {showLinkEditor ? (
            <span
              className="compose-link-popover"
              data-no-compose-drag
              onKeyDown={(event) => {
                if (event.key !== "Escape") return;
                event.preventDefault();
                event.stopPropagation();
                const trigger =
                  event.currentTarget
                    .closest(".compose-link-control")
                    ?.querySelector(".compose-format-button") || null;
                setShowLinkEditor(false);
                trigger?.focus();
              }}
            >
              <input
                ref={linkInputRef}
                aria-label="链接地址"
                value={linkValue}
                onChange={(event) => setLinkValue(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    insertLink();
                  }
                }}
                placeholder="https://"
              />
              <button
                type="button"
                onClick={insertLink}
                disabled={!safeHref(linkValue)}
              >
                添加
              </button>
              <IconButton
                label="关闭链接输入"
                onClick={() => setShowLinkEditor(false)}
              >
                <X size={14} />
              </IconButton>
            </span>
          ) : null}
        </span>
        <ToolbarButton
          label="清除格式"
          onActivate={() =>
            runEditorCommand((chain) =>
              chain.unsetAllMarks().setTextAlign("left"),
            )
          }
          disabled={disabled}
        >
          <Eraser size={17} />
        </ToolbarButton>
      </div>

      <div
        ref={editorShellRef}
        className="compose-editor-shell"
        data-stationery={stationery}
        data-paper-cell-size={paperCellSize.toFixed(2)}
        data-current-font-size={state.fontSize}
        style={{
          "--compose-paper-cell-size": `${paperCellSize}px`,
          "--compose-paper-grid-min-height": `${paperGridMinHeight}px`,
        }}
      >
        <EditorContent
          editor={editor}
          className="compose-rich-editor vertical-scroll-surface"
        />
      </div>
    </>
  );
}

export function RichTextEditor(props) {
  return (
    <RichTextEditorBoundary>
      <RichTextEditorCore {...props} />
    </RichTextEditorBoundary>
  );
}
