import {
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
    value: baseValue,
    draft: baseDraft,
    draftId: baseDraft.id,
    saveStatus: "saved",
    isSending: false,
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

afterEach(() => {
  cleanup();
  window.localStorage.clear();
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
