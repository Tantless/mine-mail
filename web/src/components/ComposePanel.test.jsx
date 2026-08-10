import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { useProseMirrorTestGeometry } from "../test/proseMirrorTestGeometry.js";
import { mailApi } from "../services/mailApi.js";
import { ComposePanel } from "./ComposePanel.jsx";

useProseMirrorTestGeometry();

const baseValue = {
  to: ["friend@example.com"],
  cc: [],
  bcc: [],
  subject: "版本化附件",
  body_text: "这是新写的正文",
  format: {
    body_html: null,
    stationery: "none",
    send_stationery: false,
  },
  reply_context: null,
};

const baseDraft = {
  id: "draft-opaque",
  local_version: 7,
  attachments: [],
  forward_context: null,
};

function renderCompose(overrides = {}) {
  const props = {
    accountId: "demo-primary",
    value: baseValue,
    draft: baseDraft,
    draftId: baseDraft.id,
    saveStatus: "saved",
    onClose: vi.fn(),
    onDiscard: vi.fn(),
    onChange: vi.fn(),
    onSaveDraft: vi.fn(),
    onRequestSend: vi.fn(),
    sendShortcut: "Ctrl ↵",
    ...overrides,
  };
  return { ...render(<ComposePanel {...props} />), props };
}

function motionRect(left, top, width, height) {
  return {
    x: left,
    y: top,
    left,
    top,
    right: left + width,
    bottom: top + height,
    width,
    height,
    toJSON: () => ({}),
  };
}

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it("summarizes minimized drafts from subject and first recipient", () => {
  const emptyValue = {
    ...baseValue,
    to: [],
    subject: "",
  };
  const contacts = [
    {
      email: "friend@example.com",
      displayName: "林夏",
    },
  ];
  const view = renderCompose({
    value: emptyValue,
    initiallyMinimized: true,
    contacts,
  });

  expect(
    screen.getByRole("button", { name: "还原写信窗口：新草稿" }).textContent,
  ).toBe("新草稿");

  view.rerender(
    <ComposePanel
      {...view.props}
      value={{ ...emptyValue, to: ["friend@example.com"] }}
    />,
  );
  expect(
    screen.getByRole("button", {
      name: "还原写信窗口：新草稿(林夏)",
    }).textContent,
  ).toBe("新草稿(林夏)");

  view.rerender(
    <ComposePanel
      {...view.props}
      value={{ ...emptyValue, subject: "季度计划" }}
    />,
  );
  expect(
    screen.getByRole("button", {
      name: "还原写信窗口：季度计划",
    }).textContent,
  ).toBe("季度计划");

  view.rerender(
    <ComposePanel
      {...view.props}
      value={{
        ...emptyValue,
        to: ["friend@example.com"],
        subject: "季度计划",
      }}
    />,
  );
  expect(
    screen.getByRole("button", {
      name: "还原写信窗口：季度计划(林夏)",
    }).textContent,
  ).toBe("季度计划(林夏)");
});

it("opens the AI assistant when the compose preference requests it", async () => {
  renderCompose({ defaultAiAssistantOpen: true });

  const assistant = await screen.findByRole("complementary", {
    name: "AI 助理",
  });
  expect(
    within(assistant).getByRole("button", { name: "收起 AI 助理" }),
  ).toBeTruthy();
});

it("coalesces drag and resize pointer samples into one geometry write per frame", () => {
  window.localStorage.setItem(
    "mine-mail-compose-geometry-v1",
    JSON.stringify({ x: 180, y: 110, width: 700, height: 540 }),
  );
  renderCompose();
  const dialog = screen.getByRole("dialog", { name: "编辑草稿" });
  const dragSurface = dialog.querySelector(".compose-drag-surface");
  const resizeHandle = dialog.querySelector('[data-resize-direction="se"]');
  const frames = new Map();
  let nextFrameId = 1;
  const requestFrame = vi
    .spyOn(window, "requestAnimationFrame")
    .mockImplementation((callback) => {
      const id = nextFrameId;
      nextFrameId += 1;
      frames.set(id, callback);
      return id;
    });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
    frames.delete(id);
  });
  const runFrame = () => {
    const [id, callback] = frames.entries().next().value;
    frames.delete(id);
    act(() => callback(16));
  };

  const initialLeft = Number.parseFloat(dialog.style.left);
  const initialTop = Number.parseFloat(dialog.style.top);
  fireEvent.pointerDown(dragSurface, {
    button: 0,
    clientX: 400,
    clientY: 120,
  });
  fireEvent.pointerMove(window, { clientX: 420, clientY: 135 });
  fireEvent.pointerMove(window, { clientX: 470, clientY: 165 });

  expect(requestFrame).toHaveBeenCalledOnce();
  expect(dialog.style.translate).toBe("");
  runFrame();
  expect(dialog.style.translate).toBe("70px 45px");

  fireEvent.pointerUp(window);
  expect(dialog.style.translate).toBe("");
  expect(Number.parseFloat(dialog.style.left)).toBe(initialLeft + 70);
  expect(Number.parseFloat(dialog.style.top)).toBe(initialTop + 45);

  const initialWidth = Number.parseFloat(dialog.style.width);
  const initialHeight = Number.parseFloat(dialog.style.height);
  fireEvent.pointerDown(resizeHandle, {
    button: 0,
    clientX: 800,
    clientY: 620,
  });
  fireEvent.pointerMove(window, { clientX: 810, clientY: 630 });
  fireEvent.pointerMove(window, { clientX: 840, clientY: 650 });

  expect(requestFrame).toHaveBeenCalledTimes(2);
  runFrame();
  fireEvent.pointerUp(window);
  expect(Number.parseFloat(dialog.style.width)).toBeGreaterThan(initialWidth);
  expect(Number.parseFloat(dialog.style.height)).toBeGreaterThan(initialHeight);

  const persisted = JSON.parse(
    window.localStorage.getItem("mine-mail-compose-geometry-v1"),
  );
  expect(persisted).toEqual({
    x: Math.round(Number.parseFloat(dialog.style.left)),
    y: Math.round(Number.parseFloat(dialog.style.top)),
    width: Math.round(Number.parseFloat(dialog.style.width)),
    height: Math.round(Number.parseFloat(dialog.style.height)),
  });
});

it("runs minimize and restore geometry through one interruptible transform transition", () => {
  renderCompose();
  const dialog = screen.getByRole("dialog", { name: "编辑草稿" });
  const layer = dialog.closest(".compose-layer");
  const expandedRect = motionRect(80, 60, 900, 720);
  const minimizedRect = motionRect(360, 700, 340, 44);
  const rect = vi.spyOn(dialog, "getBoundingClientRect");
  const frames = new Map();
  let nextFrameId = 1;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    const id = nextFrameId;
    nextFrameId += 1;
    frames.set(id, callback);
    return id;
  });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
    frames.delete(id);
  });
  const runFrame = () => {
    const [id, callback] = frames.entries().next().value;
    frames.delete(id);
    act(() => callback(16));
  };

  rect
    .mockReturnValueOnce(expandedRect)
    .mockReturnValueOnce(minimizedRect)
    .mockReturnValue(minimizedRect);
  fireEvent.pointerDown(layer, { button: 0 });

  expect(dialog.dataset.minimized).toBe("true");
  expect(dialog.dataset.windowMotion).toBe("minimizing");
  expect(dialog.dataset.windowMotionStage).toBe("inverted");
  expect(dialog.style.transform).toContain("translate3d(");
  expect(dialog.style.transform).not.toContain("scale(1, 1)");

  runFrame();
  expect(dialog.dataset.windowMotionStage).toBe("running");
  expect(dialog.style.transform).toBe("translate3d(0, 0, 0) scale(1, 1)");
  expect(dialog.dataset.entered).toBe("true");
  fireEvent.transitionEnd(dialog, {
    propertyName: "transform",
    elapsedTime: 0.05,
  });
  expect(dialog.dataset.windowMotion).toBe("minimizing");
  fireEvent.transitionEnd(dialog, {
    propertyName: "transform",
    elapsedTime: 0.26,
  });
  expect(dialog.dataset.windowMotion).toBeUndefined();
  expect(dialog.dataset.windowMotionStage).toBeUndefined();
  expect(dialog.style.transform).toBe("");

  rect
    .mockReset()
    .mockReturnValueOnce(minimizedRect)
    .mockReturnValueOnce(expandedRect)
    .mockReturnValue(expandedRect);
  fireEvent.click(
    screen.getByRole("button", {
      name: "还原写信窗口：版本化附件(friend@example.com)",
    }),
  );
  expect(dialog.dataset.minimized).toBe("false");
  expect(dialog.dataset.windowMotion).toBe("restoring");
  runFrame();
  fireEvent.transitionEnd(dialog, {
    propertyName: "transform",
    elapsedTime: 0.26,
  });
  expect(dialog.dataset.windowMotion).toBeUndefined();
  expect(dialog.style.transform).toBe("");
});

it("collapses compose window motion atomically when reduced motion is requested", () => {
  vi.stubGlobal("matchMedia", vi.fn((query) => ({
    media: query,
    matches: query === "(prefers-reduced-motion: reduce)",
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
  })));
  const requestFrame = vi.spyOn(window, "requestAnimationFrame");
  renderCompose();
  const dialog = screen.getByRole("dialog", { name: "编辑草稿" });
  vi.spyOn(dialog, "getBoundingClientRect").mockReturnValue(
    motionRect(80, 60, 900, 720),
  );

  fireEvent.pointerDown(dialog.closest(".compose-layer"), { button: 0 });
  expect(dialog.dataset.minimized).toBe("true");
  expect(dialog.dataset.windowMotion).toBeUndefined();
  expect(dialog.style.transform).toBe("");
  expect(requestFrame).not.toHaveBeenCalled();
});

it("traps focus inside the open composer and restores the invoking control", () => {
  const opener = document.createElement("button");
  opener.textContent = "打开写信";
  document.body.append(opener);
  opener.focus();

  const view = renderCompose();
  const dialog = screen.getByRole("dialog", { name: "编辑草稿" });
  const focusable = Array.from(
    dialog.querySelectorAll(
      "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [contenteditable='true'], [tabindex]:not([tabindex='-1'])",
    ),
  ).filter((element) => element.tabIndex >= 0);
  const first = focusable[0];
  const last = focusable.at(-1);

  last.focus();
  fireEvent.keyDown(last, { key: "Tab" });
  expect(document.activeElement).toBe(first);

  first.focus();
  fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
  expect(document.activeElement).toBe(last);

  view.unmount();
  expect(document.activeElement).toBe(opener);
  opener.remove();
});

it("lets recipient and link popups consume Escape before the composer", async () => {
  const user = userEvent.setup();
  const onClose = vi.fn();
  renderCompose({
    onClose,
    contacts: [
      {
        email: "contact@example.com",
        displayName: "联系人",
      },
    ],
  });

  const recipient = screen.getByRole("combobox", { name: "收件人" });
  await user.click(recipient);
  expect(screen.getByRole("listbox", { name: "收件人联系人建议" })).toBeTruthy();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("listbox", { name: "收件人联系人建议" })).toBeNull();
  expect(onClose).not.toHaveBeenCalled();

  const linkTrigger = await screen.findByRole("button", { name: "添加链接" });
  await user.click(linkTrigger);
  const linkInput = screen.getByRole("textbox", { name: "链接地址" });
  linkInput.focus();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("textbox", { name: "链接地址" })).toBeNull();
  expect(document.activeElement).toBe(linkTrigger);
  expect(onClose).not.toHaveBeenCalled();

  await user.keyboard("{Escape}");
  expect(onClose).toHaveBeenCalledOnce();
});

it("uses compact icon controls for enabling, choosing, and sending stationery", async () => {
  const onChange = vi.fn();
  const user = userEvent.setup();
  const view = renderCompose({ onChange });

  expect(screen.queryByRole("radiogroup", { name: "信纸类型" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "启用信纸" }));

  const enableUpdate = onChange.mock.calls.at(-1)[0];
  expect(enableUpdate(baseValue).format).toEqual(
    expect.objectContaining({
      stationery: "lined",
      send_stationery: false,
    }),
  );

  const linedValue = {
    ...baseValue,
    format: {
      ...baseValue.format,
      stationery: "lined",
    },
  };
  view.rerender(
    <ComposePanel
      {...view.props}
      value={linedValue}
    />,
  );

  expect(
    screen
      .getByRole("button", { name: "关闭信纸" })
      .getAttribute("aria-pressed"),
  ).toBe("true");
  expect(
    screen.getByRole("radio", { name: "横线纸" }).getAttribute("aria-checked"),
  ).toBe("true");

  screen.getByRole("radio", { name: "横线纸" }).focus();
  await user.keyboard("{ArrowRight}");
  const gridUpdate = onChange.mock.calls.at(-1)[0];
  expect(gridUpdate(linedValue).format.stationery).toBe("grid");
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("radio", { name: "方格纸" }),
    ),
  );

  await user.click(screen.getByRole("radio", { name: "将信纸随邮件发送" }));
  const sendUpdate = onChange.mock.calls.at(-1)[0];
  expect(sendUpdate(linedValue).format.send_stationery).toBe(true);
});

it("routes editor body updates through the dedicated high-frequency callback", async () => {
  const onChange = vi.fn();
  const onBodyChange = vi.fn();
  const user = userEvent.setup();
  renderCompose({ onChange, onBodyChange });

  const editor = await screen.findByRole("textbox", { name: "邮件正文" });
  await user.click(editor);
  await user.keyboard("补充");

  await waitFor(() => expect(onBodyChange).toHaveBeenCalled());
  const update = onBodyChange.mock.calls.at(-1)[0];
  expect(update(baseValue).body_text).toContain("补充");
  expect(onChange).not.toHaveBeenCalled();
});

it("reviews an optimization before applying it and can restore the overwritten body", async () => {
  const onChange = vi.fn();
  const user = userEvent.setup();
  const runTurn = vi.spyOn(mailApi, "runAiTurn");
  renderCompose({ onChange });

  expect(screen.getByRole("button", { name: "回退上次优化" }).disabled).toBe(true);
  await user.click(screen.getByRole("button", { name: "填写优化要求" }));
  const instruction = screen.getByRole("textbox", { name: "补充优化要求" });
  await user.type(instruction, "更正式，保留原有信息");
  await user.click(screen.getByRole("button", { name: "优化当前邮件" }));

  const reviewButton = await screen.findByRole("button", { name: "查看优化结果" });
  expect(onChange).not.toHaveBeenCalled();
  expect(runTurn.mock.calls[0][0].draft_revision.length).toBeLessThanOrEqual(128);
  expect(runTurn.mock.calls[0][0].draft_revision).not.toContain(baseValue.body_text);

  await user.click(reviewButton);
  const review = screen.getByRole("dialog", { name: "优化结果对比" });
  expect(within(review).getByRole("textbox", { name: "编辑左侧原文" }).value)
    .toBe(baseValue.body_text);
  expect(within(review).getByRole("textbox", { name: "编辑右侧优化结果" }).value)
    .toContain("期待您的回复");
  expect(review.querySelectorAll('[data-changed="true"]').length).toBeGreaterThan(0);

  await user.click(within(review).getByRole("button", { name: "选用右侧结果" }));
  const confirmation = screen.getByRole("alertdialog", { name: "应用优化结果？" });
  expect(within(confirmation).getByText("您确认选用右侧的结果吗？")).toBeTruthy();
  await user.click(within(confirmation).getByRole("button", { name: "确认应用" }));

  await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
  const optimizeUpdate = onChange.mock.calls.at(-1)[0];
  const optimized = optimizeUpdate(baseValue);
  expect(optimized.subject).toBe("版本化附件");
  expect(optimized.body_text).toContain("这是新写的正文");
  expect(optimized.body_text).toContain("期待您的回复");

  const undoButton = screen.getByRole("button", { name: "回退上次优化" });
  expect(undoButton.disabled).toBe(false);
  await user.click(undoButton);
  const undoUpdate = onChange.mock.calls.at(-1)[0];
  expect(undoUpdate(optimized)).toEqual(
    expect.objectContaining({
      subject: baseValue.subject,
      body_text: baseValue.body_text,
    }),
  );
  expect(screen.getByRole("button", { name: "回退上次优化" }).disabled).toBe(true);
});

it("keeps compose interactive while optimization runs and only signals the finished result", async () => {
  let finishTurn;
  const pendingTurn = new Promise((resolve) => {
    finishTurn = resolve;
  });
  vi.spyOn(mailApi, "runAiTurn").mockReturnValueOnce(pendingTurn);
  const user = userEvent.setup();
  renderCompose();

  await user.click(screen.getByRole("button", { name: "优化当前邮件" }));
  const optimizeButton = screen.getByRole("button", { name: "优化当前邮件" });
  expect(optimizeButton.disabled).toBe(true);
  expect(optimizeButton.getAttribute("aria-busy")).toBe("true");
  expect(screen.getByRole("textbox", { name: "主题" }).disabled).toBe(false);
  expect(screen.getByRole("button", { name: "填写优化要求" }).disabled).toBe(false);

  await act(async () => {
    finishTurn({
      request_id: "ai-review-pending",
      draft: {
        ...baseValue,
        body_text: `${baseValue.body_text}\n优化完成`,
      },
      changed_fields: ["body_text"],
    });
    await pendingTurn;
  });

  expect(await screen.findByRole("button", { name: "查看优化结果" })).toBeTruthy();
  expect(screen.queryByRole("dialog", { name: "优化结果对比" })).toBeNull();
});

it("keeps the optimization prompt and pending result through compose minimization", async () => {
  let finishTurn;
  const pendingTurn = new Promise((resolve) => {
    finishTurn = resolve;
  });
  vi.spyOn(mailApi, "runAiTurn").mockReturnValueOnce(pendingTurn);
  const user = userEvent.setup();
  renderCompose();

  await user.click(screen.getByRole("button", { name: "填写优化要求" }));
  await user.type(
    screen.getByRole("textbox", { name: "补充优化要求" }),
    "帮我修改格式变得工整",
  );
  await user.click(screen.getByRole("button", { name: "优化当前邮件" }));

  const dialog = screen.getByRole("dialog", { name: "编辑草稿" });
  fireEvent.pointerDown(dialog.closest(".compose-layer"), { button: 0 });
  expect(
    screen.getByRole("button", {
      name: "还原写信窗口：版本化附件(friend@example.com)",
    }),
  ).toBeTruthy();

  await act(async () => {
    finishTurn({
      request_id: "ai-review-minimized",
      draft: {
        ...baseValue,
        body_text: `${baseValue.body_text}\n优化后的排版`,
      },
      changed_fields: ["body_text"],
    });
    await pendingTurn;
  });

  await user.click(
    screen.getByRole("button", {
      name: "还原写信窗口：版本化附件(friend@example.com)",
    }),
  );
  await user.click(screen.getByRole("button", { name: "填写优化要求" }));
  expect(
    screen.getByRole("textbox", { name: "补充优化要求" }).value,
  ).toBe("帮我修改格式变得工整");

  await user.click(screen.getByRole("button", { name: "查看优化结果" }));
  expect(
    screen.getByRole("textbox", { name: "编辑右侧优化结果" }).value,
  ).toContain("优化后的排版");
});

it("uses a bounded revision identifier when optimizing a long body", async () => {
  const runTurn = vi.spyOn(mailApi, "runAiTurn");
  const user = userEvent.setup();
  const longBody = "这是一段较长的邮件正文。".repeat(180);
  renderCompose({
    value: { ...baseValue, body_text: longBody },
  });

  await user.click(screen.getByRole("button", { name: "优化当前邮件" }));
  await screen.findByRole("button", { name: "查看优化结果" });

  const request = runTurn.mock.calls[0][0];
  expect(request.draft.compose.body_text).toBe(longBody);
  expect(request.draft_revision.length).toBeLessThanOrEqual(128);
  expect(request.draft_revision).not.toContain(longBody.slice(0, 20));
});

it("minimizes and explicitly discards a completed optimization result", async () => {
  const user = userEvent.setup();
  renderCompose();

  await user.click(screen.getByRole("button", { name: "优化当前邮件" }));
  await user.click(await screen.findByRole("button", { name: "查看优化结果" }));
  await user.click(screen.getByRole("button", { name: "暂时隐藏优化结果" }));
  expect(screen.queryByRole("dialog", { name: "优化结果对比" })).toBeNull();

  await user.click(screen.getByRole("button", { name: "查看优化结果" }));
  await user.click(screen.getByRole("button", { name: "关闭优化结果" }));
  const confirmation = screen.getByRole("alertdialog", { name: "关闭优化结果？" });
  expect(
    within(confirmation).getByText(
      "确认关闭此次优化结果吗？关闭后将无法再次查看或应用。",
    ),
  ).toBeTruthy();
  await user.click(within(confirmation).getByRole("button", { name: "确认关闭" }));
  expect(screen.getByRole("button", { name: "优化当前邮件" })).toBeTruthy();
});

it("switches between the application session list and a conversation", async () => {
  const user = userEvent.setup();
  renderCompose();

  await user.click(screen.getByRole("button", { name: "打开 AI 助理" }));
  const assistant = screen.getByRole("complementary", { name: "AI 助理" });
  expect(within(assistant).getByRole("button", { name: "收起 AI 助理" })).toBeTruthy();
  expect(within(assistant).getByRole("button", { name: "AI 助理设置" })).toBeTruthy();
  expect(within(assistant).queryByRole("button", { name: "新建会话" })).toBeNull();
  expect(within(assistant).getByText("会话")).toBeTruthy();
  expect(within(assistant).queryByText("整理一封更清晰、语气更自然的项目跟进邮件"))
    .toBeNull();
  expect(within(assistant).queryByText("1 个草稿")).toBeNull();

  const input = within(assistant).getByRole("textbox", {
    name: "向 AI 助理发送消息",
  });
  await waitFor(() => expect(input.disabled).toBe(false));
  await user.type(input, "讨论项目交付时间{Enter}");
  await waitFor(() =>
    expect(
      within(assistant).getByText(
        "这是离线界面演示；桌面版会按需读取当前草稿后回答。",
      ),
    ).toBeTruthy(),
  );
  expect(within(assistant).getAllByText("答案整理完毕")).toHaveLength(1);
  expect(
    within(assistant).getByRole("button", { name: /版本化附件.*#[0-9A-F]{8}/ }),
  ).toBeTruthy();

  await user.click(within(assistant).getByRole("button", { name: "返回会话列表" }));
  expect(within(assistant).getByText("讨论项目交付时间")).toBeTruthy();
  await user.click(
    within(assistant).getByRole("button", { name: /讨论项目交付时间/ }),
  );
  expect(within(assistant).getAllByText("讨论项目交付时间").length).toBeGreaterThan(0);
});

it("creates an AI session from the fixed composer and honors the selected mode", async () => {
  const onChange = vi.fn();
  const runAiTurn = vi.spyOn(mailApi, "runAiTurn");
  const user = userEvent.setup();
  renderCompose({ onChange });

  await user.click(screen.getByRole("button", { name: "打开 AI 助理" }));
  const assistant = screen.getByRole("complementary", { name: "AI 助理" });
  const modeSelect = within(assistant).getByRole("combobox", {
    name: "选择 Agent 模式",
  });
  await waitFor(() => expect(modeSelect.disabled).toBe(false));
  await user.click(modeSelect);
  await user.click(within(assistant).getByRole("option", { name: "邮件生成" }));
  await waitFor(() => {
    expect(modeSelect.getAttribute("aria-expanded")).toBe("false");
    expect(modeSelect.textContent).toContain("邮件生成");
    expect(document.activeElement).toBe(modeSelect);
  });
  const input = within(assistant).getByRole("textbox", { name: "向 AI 助理发送消息" });
  await user.click(input);
  await user.type(input, "写一封确认下周交付时间的邮件{Enter}");

  await waitFor(() =>
    expect(
      within(assistant).getByText("已生成邮件修改提案，请在下方分别检查并应用。"),
    ).toBeTruthy(),
  );
  expect(runAiTurn).toHaveBeenCalledWith(
    expect.objectContaining({ mode: "generate" }),
    expect.any(Function),
  );
  expect(onChange).not.toHaveBeenCalled();
  await user.click(
    within(assistant).getByRole("button", { name: "应用正文与信纸提案" }),
  );
  await waitFor(() => expect(onChange).toHaveBeenCalled());
  const generateUpdate = onChange.mock.calls.at(-1)[0];
  expect(generateUpdate(baseValue).body_text).toContain("下周交付时间");
});

it("routes an Agent turn through the model selected beside the mode", async () => {
  vi.spyOn(mailApi, "refreshAiModelCatalog").mockResolvedValue({
    models: [
      {
        providerInstanceId: "11111111-1111-4111-8111-111111111111",
        providerName: "Work OpenAI",
        providerId: "openai",
        modelName: "gpt-5.6-terra",
        isDefault: true,
      },
      {
        providerInstanceId: "22222222-2222-4222-8222-222222222222",
        providerName: "Backup Kimi",
        providerId: "kimi",
        modelName: "kimi-k2.5",
        isDefault: false,
      },
    ],
    successfulProviderCount: 2,
    totalProviderCount: 3,
  });
  const runAiTurn = vi.spyOn(mailApi, "runAiTurn");
  const user = userEvent.setup();
  renderCompose();

  await user.click(screen.getByRole("button", { name: "打开 AI 助理" }));
  const assistant = screen.getByRole("complementary", { name: "AI 助理" });
  const modelSelect = within(assistant).getByRole("combobox", {
    name: "选择 AI 模型",
  });
  await waitFor(() => expect(modelSelect.disabled).toBe(false));
  expect(modelSelect.textContent).toContain("gpt-5.6-terra · Work OpenAI");

  await user.click(modelSelect);
  await user.click(
    within(assistant).getByRole("option", {
      name: "kimi-k2.5 · Backup Kimi",
    }),
  );
  const input = within(assistant).getByRole("textbox", {
    name: "向 AI 助理发送消息",
  });
  await user.type(input, "检查语气{Enter}");

  await waitFor(() => expect(runAiTurn).toHaveBeenCalled());
  expect(runAiTurn).toHaveBeenCalledWith(
    expect.objectContaining({
      provider_instance_id: "22222222-2222-4222-8222-222222222222",
      model_name: "kimi-k2.5",
    }),
    expect.any(Function),
  );
});

it("keeps a streamed Markdown turn alive while the AI sidebar is collapsed", async () => {
  let emit;
  let finish;
  const runAiTurn = vi.spyOn(mailApi, "runAiTurn").mockImplementation(
    (_request, onEvent) => new Promise((resolve) => {
      emit = onEvent;
      finish = resolve;
    }),
  );
  const user = userEvent.setup();
  renderCompose();

  await user.click(screen.getByRole("button", { name: "打开 AI 助理" }));
  const assistant = screen.getByRole("complementary", { name: "AI 助理" });
  const input = within(assistant).getByRole("textbox", { name: "向 AI 助理发送消息" });
  await waitFor(() => expect(input.disabled).toBe(false));
  await user.type(input, "检查当前草稿{Enter}");
  await waitFor(() => expect(runAiTurn).toHaveBeenCalledTimes(1));

  const streamingSession = {
    id: "session-stream",
    title: "检查当前草稿",
    lastActive: "刚刚",
    drafts: [],
    loaded: true,
    messages: [
      { id: "user-stream", role: "user", content: "检查当前草稿", status: "completed" },
      { id: "assistant-stream", role: "assistant", content: "", status: "streaming" },
    ],
  };
  await act(async () => {
    emit({ type: "started", request_id: "request-stream", session: streamingSession });
    emit({
      type: "thinking_started",
      request_id: "request-stream",
      activity_id: "thinking-1",
    });
    emit({
      type: "reasoning_delta",
      request_id: "request-stream",
      activity_id: "thinking-1",
      delta: "正在检查当前草稿",
    });
  });
  expect(within(assistant).getByText("正在检查当前草稿")).toBeTruthy();
  await act(async () => {
    emit({
      type: "tool_preparing",
      request_id: "request-stream",
      thinking_activity_id: "thinking-1",
      activity_id: "tool-1",
      name: "get_draft_body",
      display_name: "读取草稿正文",
    });
  });
  expect(within(assistant).queryByText("正在检查当前草稿")).toBeNull();
  expect(within(assistant).getByText("分析完成")).toBeTruthy();
  expect(
    within(assistant).getByText("正在调用「读取草稿正文」工具…"),
  ).toBeTruthy();
  await act(async () => {
    emit({
      type: "thinking_finished",
      request_id: "request-stream",
      activity_id: "thinking-1",
      summary: "分析完成",
      success: true,
    });
    emit({
      type: "tool_started",
      request_id: "request-stream",
      activity_id: "tool-1",
      name: "get_draft_body",
      display_name: "读取草稿正文",
    });
    emit({
      type: "tool_finished",
      request_id: "request-stream",
      activity_id: "tool-1",
      name: "get_draft_body",
      display_name: "读取草稿正文",
      success: true,
    });
    emit({
      type: "thinking_started",
      request_id: "request-stream",
      activity_id: "thinking-2",
    });
    emit({
      type: "reasoning_delta",
      request_id: "request-stream",
      activity_id: "thinking-2",
      delta: "正在整理最终答案",
    });
    emit({ type: "content_delta", request_id: "request-stream", delta: "**草稿**" });
  });
  expect(within(assistant).queryByText("正在检查当前草稿")).toBeNull();
  expect(within(assistant).getByText("分析完成")).toBeTruthy();
  expect(within(assistant).getByText("已调用「读取草稿正文」工具")).toBeTruthy();
  expect(within(assistant).getByText("正在整理最终答案")).toBeTruthy();
  expect(within(assistant).getByText("草稿").tagName).toBe("STRONG");
  expect(input.disabled).toBe(false);

  await user.click(within(assistant).getByRole("button", { name: "收起 AI 助理" }));
  expect(screen.queryByRole("complementary", { name: "AI 助理" })).toBeNull();
  const completedSession = {
    ...streamingSession,
    messages: [
      streamingSession.messages[0],
      {
        ...streamingSession.messages[1],
        content: "**草稿** 已检查",
        status: "completed",
        activities: [
          {
            id: "thinking-1",
            kind: "thinking",
            label: "分析完成",
            status: "completed",
            success: true,
            detail: "",
          },
          {
            id: "tool-1",
            kind: "tool",
            label: "已调用「读取草稿正文」工具",
            status: "completed",
            success: true,
            detail: "",
          },
          {
            id: "thinking-2",
            kind: "thinking",
            label: "答案整理完毕",
            status: "completed",
            success: true,
            detail: "",
          },
        ],
      },
    ],
  };
  await act(async () => {
    emit({ type: "content_delta", request_id: "request-stream", delta: " 已检查" });
    emit({
      type: "thinking_finished",
      request_id: "request-stream",
      activity_id: "thinking-2",
      summary: "答案整理完毕",
      success: true,
    });
    emit({ type: "completed", request_id: "request-stream" });
    finish({
      request_id: "request-stream",
      session: completedSession,
      assistant_message: "**草稿** 已检查",
      draft: null,
      changed_fields: [],
      status: "completed",
    });
  });

  await user.click(screen.getByRole("button", { name: "打开 AI 助理" }));
  expect(
    within(screen.getByRole("complementary", { name: "AI 助理" })).getByText("已检查", {
      exact: false,
    }),
  ).toBeTruthy();
});

it("stops a streamed AI turn and keeps the received partial answer", async () => {
  let emit;
  let finish;
  vi.spyOn(mailApi, "runAiTurn").mockImplementation(
    (_request, onEvent) => new Promise((resolve) => {
      emit = onEvent;
      finish = resolve;
    }),
  );
  const cancel = vi.spyOn(mailApi, "cancelAiTurn").mockImplementation(async () => {
    emit({ type: "stopped", request_id: "request-stop" });
    finish({
      request_id: "request-stop",
      session: {
        id: "session-stop",
        title: "停止测试",
        lastActive: "刚刚",
        drafts: [],
        loaded: true,
        messages: [
          { id: "user-stop", role: "user", content: "停止测试", status: "completed" },
          { id: "assistant-stop", role: "assistant", content: "部分回答", status: "stopped" },
        ],
      },
      assistant_message: "部分回答",
      draft: null,
      changed_fields: [],
      status: "stopped",
    });
    return true;
  });
  const user = userEvent.setup();
  renderCompose();
  await user.click(screen.getByRole("button", { name: "打开 AI 助理" }));
  const assistant = screen.getByRole("complementary", { name: "AI 助理" });
  const input = within(assistant).getByRole("textbox", { name: "向 AI 助理发送消息" });
  await waitFor(() => expect(input.disabled).toBe(false));
  await user.type(input, "停止测试{Enter}");
  await act(async () => {
    emit({
      type: "started",
      request_id: "request-stop",
      session: {
        id: "session-stop",
        title: "停止测试",
        lastActive: "刚刚",
        drafts: [],
        loaded: true,
        messages: [
          { id: "user-stop", role: "user", content: "停止测试", status: "completed" },
          { id: "assistant-stop", role: "assistant", content: "", status: "streaming" },
        ],
      },
    });
    emit({ type: "content_delta", request_id: "request-stop", delta: "部分回答" });
  });
  await user.click(within(assistant).getByRole("button", { name: "停止 AI 助理" }));
  await waitFor(() => expect(cancel).toHaveBeenCalledWith("request-stop"));
  expect(within(assistant).getByText("部分回答")).toBeTruthy();
  expect(within(assistant).getByText("已停止")).toBeTruthy();
});

it("renders only authoritative draft attachments and passes the exact local version", async () => {
  const onAddAttachments = vi.fn();
  const onRemoveAttachment = vi.fn();
  const user = userEvent.setup();
  renderCompose({
    value: {
      ...baseValue,
      attachments: [
        {
          id: "editable-only",
          name: "不能作为权威数据.txt",
          mime_type: "text/plain",
          size_bytes: 1,
        },
      ],
    },
    draft: {
      ...baseDraft,
      attachments: [
        {
          id: "managed-1",
          name: "安全名称.txt",
          mime_type: "text/plain",
          size_bytes: 1537,
        },
      ],
    },
    onAddAttachments,
    onRemoveAttachment,
  });

  expect(screen.getByText("安全名称.txt")).toBeTruthy();
  expect(screen.getByText(/text\/plain · 1,537 字节/)).toBeTruthy();
  expect(screen.queryByText("不能作为权威数据.txt")).toBeNull();

  await user.click(screen.getByRole("button", { name: "添加附件" }));
  expect(onAddAttachments).toHaveBeenCalledWith(7);

  const remove = screen.getByRole("button", {
    name: "移除附件 安全名称.txt",
  });
  remove.focus();
  await user.keyboard("{Enter}");
  expect(onRemoveAttachment).toHaveBeenCalledWith("managed-1", 7);
});

it.each([
  ["saved", "附件已添加"],
  ["saving", "正在添加附件…"],
  ["stale", "草稿已更新，本次操作未生效。请在最新版本重试"],
  ["conflict_copy", "附件已保存到冲突副本，请检查后继续"],
  ["error", "添加附件失败，请重试"],
  ["canceled", "已取消添加附件"],
])("renders the controlled add state %s", (status, copy) => {
  renderCompose({
    attachmentOperations: { add: { status } },
    onAddAttachments: vi.fn(),
  });

  expect(screen.getByRole("status").textContent).toBe(copy);
  if (status === "saving") {
    expect(screen.getByRole("button", { name: "正在添加附件" }).disabled).toBe(
      true,
    );
    expect(screen.getByRole("button", { name: "发送邮件" }).disabled).toBe(true);
  }
});

it.each([
  ["saving", "正在移除附件…"],
  ["stale", "草稿已更新，本次操作未生效。请在最新版本重试"],
  ["conflict_copy", "附件已保存到冲突副本，请检查后继续"],
  ["error", "移除附件失败，请重试"],
  ["canceled", "已取消移除附件"],
])("renders the controlled remove state %s", (status, copy) => {
  renderCompose({
    draft: {
      ...baseDraft,
      attachments: [
        {
          id: "managed-1",
          name: "状态附件.bin",
          mime_type: "application/octet-stream",
          size_bytes: 42,
        },
      ],
    },
    attachmentOperations: {
      remove: { "managed-1": { status } },
    },
    onRemoveAttachment: vi.fn(),
  });

  const row = screen.getByText("状态附件.bin").closest("li");
  expect(row.dataset.state).toBe(status);
  expect(within(row).getByRole("status").textContent).toBe(copy);
  if (status === "saving") {
    expect(
      within(row).getByRole("button", {
        name: "正在移除附件 状态附件.bin",
      }).disabled,
    ).toBe(true);
  }
});

it("keeps a completed remove status visible after the authoritative list updates", () => {
  renderCompose({
    attachmentOperations: {
      remove: { "managed-1": { status: "saved" } },
    },
    onRemoveAttachment: vi.fn(),
  });

  expect(screen.getByRole("status").textContent).toBe("附件已移除");
});

it("allows the owner to stabilize a new composer before opening the picker", async () => {
  const onAddAttachments = vi.fn();
  const user = userEvent.setup();
  renderCompose({
    draft: {
      id: null,
      local_version: null,
      attachments: [],
      forward_context: null,
    },
    draftId: null,
    onAddAttachments,
  });

  const add = screen.getByRole("button", { name: "添加附件" });
  expect(add.disabled).toBe(false);
  await user.click(add);
  expect(onAddAttachments).toHaveBeenCalledWith(null);
  expect(screen.queryByText(/请先保存草稿/)).toBeNull();
});

it("does not render dead attachment controls when callbacks are absent", () => {
  renderCompose({
    draft: {
      ...baseDraft,
      attachments: [
        {
          id: "managed-1",
          name: "只读元数据.pdf",
          mime_type: "application/pdf",
          size_bytes: 2048,
        },
      ],
    },
  });

  expect(screen.getByText("只读元数据.pdf")).toBeTruthy();
  expect(screen.queryByRole("button", { name: /添加附件/ })).toBeNull();
  expect(screen.queryByRole("button", { name: /移除附件/ })).toBeNull();
  expect(screen.queryByText(/尚未实现/)).toBeNull();
});

it("keeps forward identity, body, and source inventory immutable and never exposes Bcc", async () => {
  const user = userEvent.setup();
  renderCompose({
    draft: {
      ...baseDraft,
      attachments: [],
      forward_context: {
        source_message_id: "message-opaque",
        original_subject: "完整原主题",
        from: { name: "原发件人", email: "sender@example.com" },
        to: [{ name: "原收件人", email: "recipient@example.com" }],
        cc: [{ name: "抄送人", email: "copy@example.com" }],
        bcc: [{ name: "不可显示", email: "secret@example.com" }],
        sent_at: "2026-07-28T09:30:00Z",
        quoted_text: "这是完整可信的原邮件正文。",
        quoted_html: null,
        quoted_render_mode: "plain_text",
        source_attachments: [
          {
            id: "source-part-1",
            safe_display_name: "原始附件.zip",
            mime_type: "application/zip",
            size_bytes: 4097,
            disposition: "attachment",
          },
        ],
      },
    },
    forwardWarnings: ["attachments_omitted_by_user"],
    onRemoveAttachment: vi.fn(),
  });

  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  expect(editor.textContent).toBe("这是新写的正文");
  expect(editor.textContent).not.toContain("完整可信的原邮件正文");
  expect(
    screen.getByText("无附件转发：原邮件附件未加入当前草稿。"),
  ).toBeTruthy();

  const context = screen.getByLabelText("不可编辑的转发原文");
  expect(context.dataset.immutable).toBe("true");
  await user.click(
    within(context).getByRole("button", { name: /完整原主题/ }),
  );

  expect(
    context.querySelector(".compose-reply-context__plain").textContent,
  ).toBe("这是完整可信的原邮件正文。");
  expect(
    within(context).getByText(
      "原文以完整可信文本只读呈现，不会进入编辑区",
    ),
  ).toBeTruthy();
  expect(within(context).getAllByText(/原发件人/).length).toBeGreaterThan(0);
  expect(within(context).getAllByText(/原收件人/).length).toBeGreaterThan(0);
  expect(within(context).getByText(/抄送人/)).toBeTruthy();
  expect(within(context).getByText("原邮件附件（未随信发送）")).toBeTruthy();
  expect(within(context).getByText("原始附件.zip")).toBeTruthy();
  expect(within(context).getByText(/application\/zip · 4,097 字节/)).toBeTruthy();
  expect(context.textContent).not.toContain("secret@example.com");
  expect(context.textContent).not.toContain("密送");
});

it("coexists with rich text, stationery, safe HTML context, and removable forwarded attachments", async () => {
  const onRemoveAttachment = vi.fn();
  const user = userEvent.setup();
  renderCompose({
    value: {
      ...baseValue,
      format: {
        body_html: "<p>这是新写的<strong>富文本</strong></p>",
        stationery: "lined",
        send_stationery: true,
      },
    },
    draft: {
      ...baseDraft,
      attachments: [
        {
          id: "forwarded-1",
          name: "转发附件.docx",
          mime_type:
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
          size_bytes: 8192,
          source_attachment_id: "source-part-1",
        },
      ],
      forward_context: {
        source_message_id: "message-opaque",
        original_subject: "HTML 原邮件",
        from: { name: null, email: "sender@example.com" },
        to: [{ name: null, email: "friend@example.com" }],
        cc: [],
        sent_at: "2026-07-28T09:30:00Z",
        quoted_text: "可信文本回退",
        quoted_html: "<p>经过处理的 <strong>HTML 原文</strong></p>",
        quoted_render_mode: "native_html",
        source_attachments: [],
      },
    },
    onRemoveAttachment,
  });

  const editor = screen.getByRole("textbox", { name: "邮件正文" });
  expect(editor.closest(".compose-editor-shell").dataset.stationery).toBe(
    "lined",
  );
  expect(editor.textContent).toBe("这是新写的富文本");
  expect(screen.getByText(/转发附件\.docx/)).toBeTruthy();
  expect(screen.getByText(/原邮件附件/)).toBeTruthy();

  const context = screen.getByLabelText("不可编辑的转发原文");
  await user.click(
    within(context).getByRole("button", { name: /HTML 原邮件/ }),
  );
  expect(
    within(context).getByText(
      "原文以经过安全处理的 HTML 只读呈现，不会进入编辑区",
    ),
  ).toBeTruthy();
  expect(within(context).getByText("HTML 原文")).toBeTruthy();

  await user.click(
    screen.getByRole("button", { name: "移除附件 转发附件.docx" }),
  );
  expect(onRemoveAttachment).toHaveBeenCalledWith("forwarded-1", 7);
});
