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

  await waitFor(() => expect(editor.innerHTML).toContain("<strong>正文</strong>"));
  expect(onChange).toHaveBeenLastCalledWith(
    expect.objectContaining({
      format: expect.objectContaining({
        body_html: expect.stringContaining("<strong>正文</strong>"),
      }),
    }),
  );
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
