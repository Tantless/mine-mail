import {
  act,
  cleanup,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Editor } from "@tiptap/core";
import { lazy, StrictMode, Suspense, useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { useProseMirrorTestGeometry } from "../test/proseMirrorTestGeometry.js";
import {
  RichTextEditor,
  composeInputRuleExtensions,
  composeHtmlToEditorHtml,
  createComposeEditorExtensions,
  getComposeToolbarState,
  groupGridTextTokens,
  normalizeComposeHtml,
  plainTextFromDocument,
} from "./RichTextEditor.jsx";

const emptyFormat = {
  body_html: null,
  stationery: "none",
  send_stationery: false,
};

function createEngine(content) {
  return new Editor({
    extensions: createComposeEditorExtensions(),
    enableInputRules: composeInputRuleExtensions,
    content,
  });
}

function sendTextInput(editor, text) {
  for (const character of text) {
    const { from, to } = editor.state.selection;
    let handled = false;
    editor.view.someProp("handleTextInput", (handler) => {
      if (handler(editor.view, from, to, character)) {
        handled = true;
        return true;
      }
      return false;
    });
    if (!handled) editor.view.dispatch(editor.state.tr.insertText(character));
  }
}

function sendEnter(editor) {
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    code: "Enter",
    key: "Enter",
  });
  Object.defineProperties(event, {
    keyCode: { value: 13 },
    which: { value: 13 },
  });
  let handled = false;
  editor.view.someProp("handleKeyDown", (handler) => {
    if (handler(editor.view, event)) {
      handled = true;
      return true;
    }
    return false;
  });
  return handled;
}

function sendTab(editor, { shiftKey = false } = {}) {
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    code: "Tab",
    key: "Tab",
    shiftKey,
  });
  Object.defineProperties(event, {
    keyCode: { value: 9 },
    which: { value: 9 },
  });
  let handled = false;
  editor.view.someProp("handleKeyDown", (handler) => {
    if (handler(editor.view, event)) {
      handled = true;
      return true;
    }
    return false;
  });
  return handled;
}

useProseMirrorTestGeometry();

afterEach(() => {
  window.getSelection()?.removeAllRanges();
  cleanup();
  vi.restoreAllMocks();
});

it("survives lazy StrictMode lifecycle reconnection when compose opens", async () => {
  const LazyEditor = lazy(async () => ({ default: RichTextEditor }));
  const view = render(
    <StrictMode>
      <Suspense fallback={<span>正在载入编辑器…</span>}>
        <LazyEditor
          bodyText=""
          format={emptyFormat}
          stationery="none"
          onChange={vi.fn()}
        />
      </Suspense>
    </StrictMode>,
  );

  expect(
    await screen.findByRole("textbox", { name: "邮件正文" }),
  ).toBeTruthy();
  expect(screen.queryByRole("alert")).toBeNull();

  view.unmount();
});

it("keeps initialization and semantically equivalent controlled content silent", async () => {
  const onChange = vi.fn();
  const view = render(
    <RichTextEditor
      bodyText=""
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
    />,
  );

  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  await waitFor(() => expect(editor.textContent).toBe(""));
  expect(editor.parentElement?.hasAttribute("data-placeholder")).toBe(false);
  expect(editor.parentElement?.hasAttribute("data-empty")).toBe(false);
  expect(onChange).not.toHaveBeenCalled();

  view.rerender(
    <RichTextEditor
      bodyText="受控正文"
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
    />,
  );
  await waitFor(() => expect(editor.textContent).toBe("受控正文"));
  expect(onChange).not.toHaveBeenCalled();

  view.rerender(
    <RichTextEditor
      bodyText=""
      format={{ ...emptyFormat, body_html: "<p><br></p>" }}
      stationery="none"
      onChange={onChange}
    />,
  );
  await waitFor(() => expect(editor.textContent).toBe(""));
  expect(onChange).not.toHaveBeenCalled();
});

it("emits each authored document change once and keeps an empty body null", async () => {
  const onChange = vi.fn();
  const onEditorReady = vi.fn();
  const view = render(
    <RichTextEditor
      bodyText=""
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
      onEditorReady={onEditorReady}
    />,
  );
  const editor = onEditorReady.mock.calls.find(([candidate]) => candidate)?.[0];
  expect(editor).toBeTruthy();
  expect(onChange).not.toHaveBeenCalled();

  act(() => editor.commands.insertContent("用户输入"));
  await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
  expect(onChange).toHaveBeenLastCalledWith({
    body_text: "用户输入",
    format: {
      ...emptyFormat,
      body_html: "<p>用户输入</p>",
    },
  });

  act(() => editor.commands.clearContent());
  await waitFor(() => expect(onChange).toHaveBeenCalledTimes(2));
  expect(onChange).toHaveBeenLastCalledWith({
    body_text: "",
    format: {
      ...emptyFormat,
      body_html: null,
    },
  });

  await act(async () => Promise.resolve());
  expect(onChange).toHaveBeenCalledTimes(2);
  view.unmount();
});

it("preserves consecutive authored input in a controlled compose value", async () => {
  const onAuthoredChange = vi.fn();
  const user = userEvent.setup();

  function ControlledEditor() {
    const [value, setValue] = useState({
      body_text: "",
      format: emptyFormat,
    });
    return (
      <RichTextEditor
        bodyText={value.body_text}
        format={value.format}
        stationery="none"
        onChange={(next) => {
          onAuthoredChange(next);
          setValue(next);
        }}
      />
    );
  }

  render(<ControlledEditor />);
  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  await user.type(editor, "这是回复内容");

  expect(
    onAuthoredChange.mock.calls.map(([next]) => next.body_text),
  ).toEqual([
    "这",
    "这是",
    "这是回",
    "这是回复",
    "这是回复内",
    "这是回复内容",
  ]);
  expect(editor.textContent).toBe("这是回复内容");
  expect(onAuthoredChange).toHaveBeenCalledTimes(6);
});

it("normalizes semantically empty compose HTML to the empty contract", () => {
  expect(normalizeComposeHtml("<p></p>")).toBe("");
  expect(normalizeComposeHtml("<p><br></p>")).toBe("");
  expect(normalizeComposeHtml("<div>&nbsp;</div>")).toBe("");
});

it("keeps only the editor formatting allowlist and converts safe text styles", () => {
  const cleaned = normalizeComposeHtml(
    '<div align="center" onclick="bad()"><b>粗体</b><strong>安全</strong><span style="font-family: KaiTi; font-size: 18px; color: red">楷体</span><script>bad()</script><a href="javascript:bad()">链接</a></div>',
  );

  expect(cleaned).toContain('align="center"');
  expect(cleaned).toContain("<strong>粗体</strong>");
  expect(cleaned).toContain("<strong>安全</strong>");
  expect(cleaned).toContain('<font face="KaiTi" size="5">楷体</font>');
  expect(cleaned).not.toContain("onclick");
  expect(cleaned).not.toContain("color:");
  expect(cleaned).not.toContain("script");
  expect(cleaned).not.toContain("javascript:");

  expect(
    composeHtmlToEditorHtml('<p><font face="SimSun" size="4">正文</font></p>'),
  ).toContain('style="font-family: SimSun; font-size: 16px;"');
});

it("keeps semantic strike and canonicalizes bundled font fallback stacks", () => {
  const cleaned = normalizeComposeHtml(
    '<p><s>旧内容</s><span style="font-family: Ma Shan Zheng">书写正文</span><span style="font-family: FangSong">仿宋正文</span></p>',
  );

  expect(cleaned).toContain("<s>旧内容</s>");
  expect(cleaned).toContain(
    'face="Ma Shan Zheng,Zhi Mang Xing,STXingkai,cursive"',
  );
  expect(cleaned).toContain(
    'face="FangSong,STFangsong,Noto Serif SC Variable,serif"',
  );
});

it("keeps only the fixed semantic first-line indent", () => {
  const cleaned = normalizeComposeHtml(
    '<p data-first-line-indent="tab" style="text-indent: 4em; color: red">缩进</p><p data-first-line-indent="bad" style="text-indent: 9em">普通</p>',
  );

  expect(cleaned).toContain('data-first-line-indent="tab"');
  expect(cleaned).toContain("text-indent: 2em");
  expect(cleaned).not.toContain("text-indent: 4em");
  expect(cleaned).not.toContain("color:");
  expect(cleaned).not.toContain("9em");
  expect(cleaned).not.toContain('data-first-line-indent="bad"');
});

it("formats the active selection without the deprecated browser command API", async () => {
  const onChange = vi.fn();
  const onEditorReady = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText="正文"
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
      onEditorReady={onEditorReady}
    />,
  );
  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  const engine = onEditorReady.mock.calls.at(-1)[0];
  engine.commands.setTextSelection({ from: 1, to: 3 });

  await user.click(screen.getByRole("button", { name: "粗体" }));

  await waitFor(() => {
    const strong = editor.querySelector("strong");
    expect(strong?.textContent).toBe("正文");
  });
  expect(onChange).toHaveBeenLastCalledWith(
    expect.objectContaining({
      format: expect.objectContaining({
        body_html: expect.stringContaining("<strong>正文</strong>"),
      }),
    }),
  );
});

it("absolutizes protocol-relative links when inserting a link", async () => {
  const onChange = vi.fn();
  const onEditorReady = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText="正文"
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
      onEditorReady={onEditorReady}
    />,
  );
  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  const engine = onEditorReady.mock.calls.at(-1)[0];
  engine.commands.setTextSelection({ from: 1, to: 3 });

  await user.click(screen.getByRole("button", { name: "添加链接" }));
  await user.type(
    screen.getByRole("textbox", { name: "链接地址" }),
    "//example.com/page",
  );
  await user.click(screen.getByRole("button", { name: "添加" }));

  await waitFor(() => {
    const anchor = editor.querySelector("a");
    expect(anchor?.getAttribute("href")).toBe("http://example.com/page");
  });
});

it("defaults a bare hostname to https when inserting a link", async () => {
  const onChange = vi.fn();
  const onEditorReady = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText="正文"
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
      onEditorReady={onEditorReady}
    />,
  );
  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  const engine = onEditorReady.mock.calls.at(-1)[0];
  engine.commands.setTextSelection({ from: 1, to: 3 });

  await user.click(screen.getByRole("button", { name: "添加链接" }));
  await user.type(
    screen.getByRole("textbox", { name: "链接地址" }),
    "www.example.com",
  );
  await user.click(screen.getByRole("button", { name: "添加" }));

  await waitFor(() => {
    const anchor = editor.querySelector("a");
    expect(anchor?.getAttribute("href")).toBe("https://www.example.com/");
  });
});

it("keeps the link editor open with a hint when nothing is selected", async () => {
  const onEditorReady = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText="正文"
      format={emptyFormat}
      stationery="none"
      onChange={vi.fn()}
      onEditorReady={onEditorReady}
    />,
  );
  const editor = screen.getByRole("textbox", { name: "邮件正文" });

  await user.click(screen.getByRole("button", { name: "添加链接" }));
  await user.type(
    screen.getByRole("textbox", { name: "链接地址" }),
    "https://example.com",
  );
  await user.click(screen.getByRole("button", { name: "添加" }));

  await waitFor(() => {
    expect(screen.getByRole("alert").textContent).toContain(
      "选中要添加链接的文字",
    );
  });
  expect(screen.getByRole("textbox", { name: "链接地址" })).toBeTruthy();
  expect(editor.querySelector("a")).toBeNull();

  await user.click(screen.getByRole("button", { name: "关闭链接输入" }));
  expect(screen.queryByRole("alert")).toBeNull();
  await user.click(screen.getByRole("button", { name: "添加链接" }));
  expect(screen.getByRole("textbox", { name: "链接地址" }).value).toBe(
    "https://example.com",
  );
  expect(screen.queryByRole("alert")).toBeNull();
});

it("changes only the selected text size without relaying out the paper", async () => {
  const onEditorReady = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText={"第一行\n第二行"}
      format={{ ...emptyFormat, stationery: "grid" }}
      stationery="grid"
      onChange={vi.fn()}
      onEditorReady={onEditorReady}
    />,
  );

  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  const shell = editor.closest(".compose-editor-shell");
  const firstLine = editor.firstElementChild;
  const secondLine = editor.lastElementChild;
  const engine = onEditorReady.mock.calls.at(-1)[0];
  engine.commands.setTextSelection({ from: 6, to: 9 });

  expect(shell.dataset.stationery).toBe("grid");
  expect(shell.style.getPropertyValue("--compose-editor-font-size")).toBe("");
  expect(Number(shell.dataset.paperCellSize)).toBe(28);

  await user.click(screen.getByRole("combobox", { name: "字号" }));
  await user.click(screen.getByRole("option", { name: "18" }));

  await waitFor(() =>
    expect(secondLine.innerHTML).toContain('style="font-size: 18px;"'),
  );
  expect(firstLine.innerHTML).not.toContain("font-size");
  expect(shell.style.getPropertyValue("--compose-editor-font-size")).toBe("");
  expect(Number(shell.dataset.paperCellSize)).toBe(28);
});

it("previews and applies a bundled font with its safe fallback stack", async () => {
  const onChange = vi.fn();
  const onEditorReady = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText="字体正文"
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
      onEditorReady={onEditorReady}
    />,
  );
  const editor = onEditorReady.mock.calls.at(-1)[0];
  act(() => editor.commands.setTextSelection({ from: 1, to: 3 }));

  await user.click(screen.getByRole("combobox", { name: "字体" }));
  expect(
    screen
      .getAllByRole("option")
      .slice(0, 5)
      .map((item) => item.textContent),
  ).toEqual(["默认字体", "微软雅黑", "宋体", "楷体", "仿宋"]);
  const option = screen.getByRole("option", { name: "站酷小薇体" });
  expect(option.querySelector(".themed-select__option-label")?.style.fontFamily).toContain(
    "ZCOOL XiaoWei",
  );
  await user.click(option);

  await waitFor(() =>
    expect(editor.getHTML()).toContain("ZCOOL XiaoWei"),
  );
  expect(onChange.mock.calls.at(-1)?.[0].format.body_html).toContain(
    'face="ZCOOL XiaoWei,Noto Serif SC Variable,Songti SC,SimSun,serif"',
  );
});

it("groups Latin input by three cells while keeping Han and spaces independent", () => {
  expect(groupGridTextTokens("asd1a暗色 的!")).toEqual([
    { from: 0, to: 3, kind: "latin", text: "asd" },
    { from: 3, to: 5, kind: "latin", text: "1a" },
    { from: 5, to: 6, kind: "han", text: "暗" },
    { from: 6, to: 7, kind: "han", text: "色" },
    { from: 7, to: 8, kind: "space", text: " " },
    { from: 8, to: 9, kind: "han", text: "的" },
    { from: 9, to: 10, kind: "special", text: "!" },
  ]);
});

it("gives every typed grid-paper space one complete independent cell", async () => {
  const onEditorReady = vi.fn();
  render(
    <RichTextEditor
      bodyText=""
      format={{ ...emptyFormat, stationery: "grid" }}
      stationery="grid"
      onChange={vi.fn()}
      onEditorReady={onEditorReady}
    />,
  );

  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  const engine = onEditorReady.mock.calls.at(-1)[0];
  act(() => sendTextInput(engine, "a  \u00a0暗"));

  await waitFor(() => {
    const tokens = Array.from(
      editor.querySelectorAll(".compose-grid-cell-token"),
    );
    expect(tokens.map((token) => token.textContent)).toEqual([
      "a",
      " ",
      " ",
      "\u00a0",
      "暗",
    ]);
    expect(tokens.map((token) => token.dataset.gridTokenKind)).toEqual([
      "latin",
      "space",
      "space",
      "space",
      "han",
    ]);
  });
  expect(editor.textContent).toBe("a  \u00a0暗");
});

it("moves the grid-paper caret across the full blank cell", async () => {
  const onEditorReady = vi.fn();
  render(
    <RichTextEditor
      bodyText=""
      format={{ ...emptyFormat, stationery: "grid" }}
      stationery="grid"
      onChange={vi.fn()}
      onEditorReady={onEditorReady}
    />,
  );

  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  const engine = onEditorReady.mock.calls.at(-1)[0];
  act(() => sendTextInput(engine, "a "));

  await waitFor(() => {
    const space = editor.querySelector(
      '.compose-grid-cell-token[data-grid-token-kind="space"]',
    );
    const caret = editor.querySelector(".compose-grid-space-caret");
    expect(space?.textContent).toBe(" ");
    expect(caret).not.toBeNull();
    expect(space?.contains(caret)).toBe(false);
    expect(
      space?.compareDocumentPosition(caret) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(editor.querySelector("p")?.classList).toContain(
      "compose-grid-space-caret-active",
    );
  });

  act(() => onEditorReady.mock.calls.at(-1)[0].commands.focus());
  await waitFor(() => {
    const caret = editor.querySelector(".compose-grid-space-caret");
    const selectionRange = window.getSelection()?.getRangeAt(0);
    const afterCaret = document.createRange();
    afterCaret.setStartAfter(caret);
    afterCaret.collapse(true);
    expect(
      selectionRange?.compareBoundaryPoints(Range.START_TO_START, afterCaret),
    ).toBe(0);
  });

  act(() => engine.commands.setTextSelection(2));

  await waitFor(() => {
    const space = editor.querySelector(
      '.compose-grid-cell-token[data-grid-token-kind="space"]',
    );
    const caret = editor.querySelector(".compose-grid-space-caret");
    expect(caret).not.toBeNull();
    expect(space?.contains(caret)).toBe(false);
    expect(
      caret?.compareDocumentPosition(space) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(editor.querySelector("p")?.classList).toContain(
      "compose-grid-space-caret-active",
    );
  });

  act(() => engine.commands.setTextSelection(1));

  await waitFor(() => {
    expect(editor.querySelector(".compose-grid-space-caret")).toBeNull();
    expect(editor.querySelector("p")?.classList).not.toContain(
      "compose-grid-space-caret-active",
    );
  });
});

it("places text typed after a grid-paper space in the following cell", async () => {
  const onEditorReady = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText=""
      format={{ ...emptyFormat, stationery: "grid" }}
      stationery="grid"
      onChange={vi.fn()}
      onEditorReady={onEditorReady}
    />,
  );

  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  await user.click(editor);
  await user.keyboard("a {Space}暗");

  await waitFor(() => {
    const tokens = Array.from(
      editor.querySelectorAll(".compose-grid-cell-token"),
    );
    expect(tokens.map((token) => token.textContent)).toEqual(["a", " ", "暗"]);
    expect(tokens.map((token) => token.dataset.gridTokenKind)).toEqual([
      "latin",
      "space",
      "han",
    ]);
  });
  expect(onEditorReady.mock.calls.at(-1)[0].getText()).toBe("a 暗");
});

it("keeps a long legacy-indented paragraph cell-aligned after switching to grid paper", async () => {
  const text = "长段落".repeat(32);
  const format = {
    ...emptyFormat,
    body_html: `<p data-first-line-indent="tab" style="text-indent: 4em">${text}</p>`,
    stationery: "lined",
  };
  const view = render(
    <RichTextEditor
      bodyText={text}
      format={format}
      stationery="lined"
      onChange={vi.fn()}
    />,
  );
  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  expect(editor.querySelector("p")?.style.textIndent).toBe("2em");
  expect(editor.querySelectorAll(".compose-grid-cell-token")).toHaveLength(0);

  view.rerender(
    <RichTextEditor
      bodyText={text}
      format={{ ...format, stationery: "grid" }}
      stationery="grid"
      onChange={vi.fn()}
    />,
  );

  await waitFor(() => {
    expect(
      editor.closest(".compose-editor-shell")?.dataset.stationery,
    ).toBe("grid");
    expect(editor.querySelector("p")?.dataset.firstLineIndent).toBe("tab");
    expect(
      editor.querySelectorAll(".compose-grid-cell-token"),
    ).toHaveLength(Array.from(text).length);
  });
});

it("keeps Han centered after Latin text is typed into grid paper", async () => {
  const onEditorReady = vi.fn();
  render(
    <RichTextEditor
      bodyText=""
      format={{ ...emptyFormat, stationery: "grid" }}
      stationery="grid"
      onChange={vi.fn()}
      onEditorReady={onEditorReady}
    />,
  );

  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  const engine = onEditorReady.mock.calls.at(-1)[0];
  act(() => sendTextInput(engine, "asd1a暗色"));

  await waitFor(() => {
    const tokens = Array.from(
      editor.querySelectorAll(".compose-grid-cell-token"),
    );
    expect(tokens.map((token) => token.textContent)).toEqual([
      "asd",
      "1a",
      "暗",
      "色",
    ]);
    expect(tokens.map((token) => token.dataset.gridTokenKind)).toEqual([
      "latin",
      "latin",
      "han",
      "han",
    ]);
  });
  expect(editor.textContent).toBe("asd1a暗色");
});

it("uses Tab for first-line indent and inherits it across new paragraphs", () => {
  const editor = createEngine("<p>第一段</p>");
  editor.commands.setTextSelection(1);

  expect(sendTab(editor)).toBe(true);
  expect(editor.view.dom.querySelector("p")?.dataset.firstLineIndent).toBe(
    "tab",
  );
  expect(editor.getHTML()).toContain('data-first-line-indent="tab"');
  expect(editor.getHTML()).toContain("text-indent: 2em");

  editor.commands.setTextSelection(editor.state.doc.content.size - 1);
  expect(sendEnter(editor)).toBe(true);
  sendTextInput(editor, "第二段");
  expect(sendEnter(editor)).toBe(true);

  const paragraphs = Array.from(editor.view.dom.querySelectorAll("p"));
  expect(paragraphs).toHaveLength(3);
  expect(
    paragraphs.map((paragraph) => paragraph.dataset.firstLineIndent),
  ).toEqual(["tab", "tab", "tab"]);
  expect(editor.state.selection.$from.parentOffset).toBe(0);

  expect(sendTab(editor, { shiftKey: true })).toBe(true);
  expect(
    editor.view.dom.querySelectorAll("p")[2].dataset.firstLineIndent,
  ).toBeUndefined();
  editor.destroy();
});

it("keeps real keyboard focus in the editor when Tab indents a paragraph", async () => {
  const onChange = vi.fn();
  const onEditorReady = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText="第一段"
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
      onEditorReady={onEditorReady}
    />,
  );
  const editorElement = screen.getByRole("textbox", { name: "邮件正文" });
  const editor = onEditorReady.mock.calls.at(-1)[0];
  act(() => editor.commands.setTextSelection(1));
  editorElement.focus();

  await user.keyboard("{Tab}");

  expect(document.activeElement).toBe(editorElement);
  expect(editorElement.querySelector("p")?.dataset.firstLineIndent).toBe(
    "tab",
  );
  expect(onChange).toHaveBeenLastCalledWith(
    expect.objectContaining({
      format: expect.objectContaining({
        body_html: expect.stringContaining('data-first-line-indent="tab"'),
      }),
    }),
  );
});

it("does not turn Tab into paragraph indent away from the paragraph start", () => {
  const editor = createEngine("<p>正文</p>");
  editor.commands.setTextSelection(2);

  expect(sendTab(editor)).toBe(false);
  expect(editor.getHTML()).toBe("<p>正文</p>");
  editor.destroy();
});

it("keeps the active paragraph and selection after changing alignment", async () => {
  const onChange = vi.fn();
  const onEditorReady = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText={"第一行\n第二行"}
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
      onEditorReady={onEditorReady}
    />,
  );

  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  const engine = onEditorReady.mock.calls.at(-1)[0];
  engine.commands.setTextSelection(7);
  const selectionBefore = engine.state.selection.from;

  await user.click(screen.getByRole("button", { name: "居中对齐" }));

  await waitFor(() =>
    expect(editor.lastElementChild.style.textAlign).toBe("center"),
  );
  expect(engine.state.selection.from).toBe(selectionBefore);
  await waitFor(() =>
    expect(
      editor.lastElementChild.contains(window.getSelection().anchorNode),
    ).toBe(true),
  );
  expect(onChange).toHaveBeenLastCalledWith(
    expect.objectContaining({
      format: expect.objectContaining({
        body_html: expect.stringContaining(
          '<p align="center">第二行</p>',
        ),
      }),
    }),
  );
});

it("synchronizes the toolbar with the inherited font size at a collapsed caret", () => {
  const editor = createEngine(
    '<p><span style="font-size: 18px">大字</span><span style="font-size: 12px">小字</span></p>',
  );

  editor.commands.setTextSelection(3);
  expect(getComposeToolbarState(editor).fontSize).toBe("18");

  editor.commands.setTextSelection(5);
  expect(getComposeToolbarState(editor).fontSize).toBe("12");

  editor.commands.setTextSelection({ from: 1, to: 5 });
  expect(getComposeToolbarState(editor).mixedFontSize).toBe(true);

  editor.destroy();
});

it("updates the visible font controls when the caret crosses mixed formatting", async () => {
  const onEditorReady = vi.fn();
  render(
    <RichTextEditor
      bodyText="大字小字"
      format={{
        ...emptyFormat,
        body_html:
          '<p><font face="KaiTi" size="5">大字</font><font face="Consolas" size="2">小字</font></p>',
      }}
      onChange={vi.fn()}
      onEditorReady={onEditorReady}
    />,
  );
  const editor = onEditorReady.mock.calls.at(-1)[0];
  const fontControl = screen.getByRole("combobox", { name: "字体" });
  const sizeControl = screen.getByRole("combobox", { name: "字号" });

  act(() => editor.commands.setTextSelection(3));
  await waitFor(() => {
    expect(fontControl.textContent).toContain("楷体");
    expect(sizeControl.textContent).toContain("18");
  });

  act(() => editor.commands.setTextSelection(5));
  await waitFor(() => {
    expect(fontControl.textContent).toContain("等宽字体");
    expect(sizeControl.textContent).toContain("12");
  });

  act(() => editor.commands.setTextSelection({ from: 1, to: 5 }));
  await waitFor(() => {
    expect(fontControl.textContent).toContain("—");
    expect(sizeControl.textContent).toContain("—");
  });
});

it("stores a new collapsed-caret size for the caret and future input", () => {
  const editor = createEngine("<p>正文</p>");
  editor.commands.setTextSelection(3);
  editor.commands.setFontSize("22px");

  expect(getComposeToolbarState(editor).fontSize).toBe("22");
  expect(
    editor.state.storedMarks?.find((mark) => mark.type.name === "textStyle")
      ?.attrs.fontSize,
  ).toBe("22px");
  expect(
    editor.view.dom
      .querySelector(".compose-format-caret-probe")
      ?.closest("span")?.style.fontSize,
  ).toBe("22px");

  editor.commands.insertContent("续");
  expect(editor.getHTML()).toContain(
    '<span style="font-size: 22px;">续</span>',
  );
  editor.destroy();
});

it("applies real italic markup and keeps italic active at the following caret", () => {
  const editor = createEngine("<p>斜体正文</p>");
  editor.commands.setTextSelection({ from: 1, to: 3 });
  editor.commands.toggleItalic();

  expect(editor.getHTML()).toContain("<em>斜体</em>");
  editor.commands.setTextSelection(3);
  expect(getComposeToolbarState(editor).italic).toBe(true);
  editor.destroy();
});

it("toggles semantic strike from the visible toolbar without losing selection", async () => {
  const onChange = vi.fn();
  const onEditorReady = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText="旧内容正文"
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
      onEditorReady={onEditorReady}
    />,
  );
  const editor = onEditorReady.mock.calls.at(-1)[0];
  const strikeButton = screen.getByRole("button", { name: "删除线" });
  act(() => editor.commands.setTextSelection({ from: 1, to: 4 }));

  await user.click(strikeButton);

  await waitFor(() =>
    expect(strikeButton.getAttribute("aria-pressed")).toBe("true"),
  );
  expect(editor.state.selection.from).toBe(1);
  expect(editor.state.selection.to).toBe(4);
  expect(editor.getHTML()).toContain("<s>旧内容</s>");
  expect(onChange.mock.calls.at(-1)?.[0].format.body_html).toContain(
    "<s>旧内容</s>",
  );
});

it("toggles semantic italic from the visible toolbar without losing selection", async () => {
  const onChange = vi.fn();
  const onEditorReady = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText="斜体正文"
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
      onEditorReady={onEditorReady}
    />,
  );
  const editor = onEditorReady.mock.calls.at(-1)[0];
  const italicButton = screen.getByRole("button", { name: "斜体" });
  act(() => editor.commands.setTextSelection({ from: 1, to: 3 }));

  await user.click(italicButton);

  await waitFor(() =>
    expect(italicButton.getAttribute("aria-pressed")).toBe("true"),
  );
  expect(editor.state.selection.from).toBe(1);
  expect(editor.state.selection.to).toBe(3);
  expect(editor.getHTML()).toContain("<em>斜体</em>");
  expect(onChange).toHaveBeenLastCalledWith(
    expect.objectContaining({
      format: expect.objectContaining({
        body_html: expect.stringContaining("<em>斜体</em>"),
      }),
    }),
  );
});

it("recognizes numbered-list input and preserves inline typing marks", () => {
  const editor = createEngine("<p></p>");
  editor.commands.setFontSize("18px");
  expect(editor.extensionManager.splittableMarks).toContain("textStyle");

  sendTextInput(editor, "1.");
  expect(editor.getHTML()).toContain('style="font-size: 18px;"');
  expect(
    editor.state.selection.$from.nodeBefore?.marks.find(
      (mark) => mark.type.name === "textStyle",
    )?.attrs.fontSize,
  ).toBe("18px");
  sendTextInput(editor, " ");
  expect(editor.isActive("orderedList")).toBe(true);
  expect(getComposeToolbarState(editor).fontSize).toBe("18");

  sendTextInput(editor, "第一项");
  expect(sendEnter(editor)).toBe(true);
  sendTextInput(editor, "第二项");
  expect(sendEnter(editor)).toBe(true);
  expect(editor.isActive("orderedList")).toBe(true);
  expect(sendEnter(editor)).toBe(true);
  expect(editor.isActive("orderedList")).toBe(false);
  sendTextInput(editor, "后续段落");

  const html = editor.getHTML();
  expect(html).toContain("<ol>");
  expect(html.match(/<li>/g)).toHaveLength(2);
  expect(html).toContain("第一项");
  expect(html).toContain("第二项");
  expect(html).toContain("<p>后续段落</p>");
  expect(html).toContain('style="font-size: 18px;"');
  expect(plainTextFromDocument(editor.getJSON())).toBe(
    "1. 第一项\n2. 第二项\n后续段落",
  );
  editor.destroy();
});

it("supports the complete numbered-list flow through real compose keystrokes", async () => {
  const onChange = vi.fn();
  const user = userEvent.setup();
  render(
    <RichTextEditor
      bodyText=""
      format={emptyFormat}
      stationery="none"
      onChange={onChange}
    />,
  );
  const editor = screen.getByRole("textbox", { name: "邮件正文" });

  await user.type(editor, "1. 第一项{Enter}第二项{Enter}{Enter}后续段落");

  expect(editor.querySelectorAll("ol > li")).toHaveLength(2);
  const paragraphs = [...editor.querySelectorAll(":scope > p")].filter(
    (paragraph) => paragraph.textContent,
  );
  expect(paragraphs.at(-1)?.textContent).toBe("后续段落");
  expect(onChange).toHaveBeenLastCalledWith(
    expect.objectContaining({
      body_text: "1. 第一项\n2. 第二项\n后续段落",
    }),
  );
});
