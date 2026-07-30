import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MessageView } from "./MessageView.jsx";

vi.mock("./ReaderIdleExperience.jsx", () => ({
  ReaderIdleExperience: () => <div data-testid="reader-idle-experience" />,
}));

afterEach(cleanup);

function messageFixture(overrides = {}) {
  return {
    uid: 1,
    kind: "inbox",
    subject: "测试邮件",
    sender: { email: "sender@example.com", name: "Sender" },
    to: [{ name: "Mine Mail", email: "me@example.com" }],
    cc: [],
    sent_at: "2026-07-21T12:00:00Z",
    body_fetched: true,
    body_text: "完整正文",
    ...overrides,
  };
}

describe("MessageView idle experience", () => {
  it("replaces the instructional placeholder and unmounts as soon as a message opens", () => {
    const { rerender } = render(<MessageView message={null} />);

    expect(screen.getByTestId("reader-idle-experience")).toBeTruthy();
    expect(screen.queryByText("选择一封邮件开始阅读")).toBeNull();
    expect(screen.queryByText("背景会在这里多留一点呼吸")).toBeNull();

    rerender(
      <MessageView
        message={{
          uid: 1,
          kind: "inbox",
          subject: "已打开的邮件",
          sender: { email: "sender@example.com", name: "Sender" },
          sent_at: "2026-07-21T12:00:00Z",
          body_fetched: false,
          preview: "正文预览",
        }}
        onClose={vi.fn()}
      />,
    );

    expect(screen.queryByTestId("reader-idle-experience")).toBeNull();
    expect(screen.getByText("已打开的邮件")).toBeTruthy();
  });

  it("uses the vertical-only scroll contract for an opened message", () => {
    const { container } = render(
      <MessageView
        message={{
          uid: 2,
          kind: "inbox",
          subject: "仅纵向滚动",
          sender: { email: "sender@example.com", name: "Sender" },
          sent_at: "2026-07-21T12:00:00Z",
          body_fetched: true,
          body_text: "正文",
        }}
        onClose={vi.fn()}
      />,
    );

    const scrollSurface = container.querySelector(".reader-scroll");
    expect(scrollSurface).toBeTruthy();
    expect(scrollSurface.classList.contains("vertical-scroll-surface")).toBe(true);
  });
});

describe("MessageView window motion", () => {
  it("makes an exiting reader inert and completes only from its own animation", () => {
    const onMotionEnd = vi.fn();
    const { container } = render(
      <MessageView
        message={messageFixture()}
        onClose={vi.fn()}
        motion="exiting"
        exitSpeed="fast"
        onMotionEnd={onMotionEnd}
      />,
    );
    const reader = container.querySelector(".reader-panel--message");
    const child = reader.querySelector(".reader-toolbar");

    expect(reader.dataset.readerMotion).toBe("exiting");
    expect(reader.dataset.readerExitSpeed).toBe("fast");
    expect(reader.hasAttribute("inert")).toBe(true);
    expect(reader.getAttribute("aria-hidden")).toBe("true");

    // React selects the WebKit-prefixed synthetic event in jsdom because
    // AnimationEvent is absent there; production browsers still emit
    // animationend for the same onAnimationEnd prop.
    fireEvent(
      child,
      new Event("webkitAnimationEnd", {
        bubbles: true,
        cancelable: false,
      }),
    );
    expect(onMotionEnd).not.toHaveBeenCalled();
    fireEvent(
      reader,
      new Event("webkitAnimationEnd", {
        bubbles: true,
        cancelable: false,
      }),
    );
    expect(onMotionEnd).toHaveBeenCalledOnce();
  });
});

describe("MessageView role-specific actions", () => {
  it("exposes only the actions allowed for Inbox and calls their controlled callbacks", async () => {
    const user = userEvent.setup();
    const onArchive = vi.fn();
    const onMoveToTrash = vi.fn();
    const onMarkUnread = vi.fn();

    render(
      <MessageView
        message={messageFixture()}
        onClose={vi.fn()}
        onArchive={onArchive}
        onMoveToTrash={onMoveToTrash}
        onPermanentDelete={vi.fn()}
        onMarkUnread={onMarkUnread}
      />,
    );

    await user.click(screen.getByRole("button", { name: "归档" }));
    await user.click(screen.getByRole("button", { name: "移到垃圾箱" }));
    await user.click(screen.getByRole("button", { name: "标记为未读" }));

    expect(onArchive).toHaveBeenCalledOnce();
    expect(onMoveToTrash).toHaveBeenCalledOnce();
    expect(onMarkUnread).toHaveBeenCalledOnce();
    expect(
      screen.queryByRole("button", { name: "永久删除" }),
    ).toBeNull();
  });

  it.each([
    ["sent", ["移到垃圾箱", "标记为未读"], ["归档", "永久删除"]],
    ["archive", ["移到垃圾箱", "标记为未读"], ["归档", "永久删除"]],
    ["trash", ["永久删除", "标记为未读"], ["归档", "移到垃圾箱"]],
    ["draft", [], ["归档", "移到垃圾箱", "永久删除", "标记为未读"]],
    ["outbox", [], ["归档", "移到垃圾箱", "永久删除", "标记为未读"]],
  ])("uses the %s mailbox role contract", (kind, visible, hidden) => {
    render(
      <MessageView
        message={messageFixture({ kind })}
        onClose={vi.fn()}
        onArchive={vi.fn()}
        onMoveToTrash={vi.fn()}
        onPermanentDelete={vi.fn()}
        onMarkUnread={vi.fn()}
      />,
    );

    for (const name of visible) {
      expect(screen.getByRole("button", { name })).toBeTruthy();
    }
    for (const name of hidden) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
  });

  it("uses displayed_role as the authoritative action context", () => {
    render(
      <MessageView
        message={messageFixture({
          kind: "inbox",
          mailbox_role: "inbox",
          displayed_role: "trash",
        })}
        onClose={vi.fn()}
        onArchive={vi.fn()}
        onMoveToTrash={vi.fn()}
        onPermanentDelete={vi.fn()}
        onMarkUnread={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("button", { name: "永久删除" }),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "归档" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "移到垃圾箱" }),
    ).toBeNull();
  });

  it("offers only explicit delivery-unknown decisions and never ordinary retry", async () => {
    const user = userEvent.setup();
    const onResolveDeliveryUnknown = vi.fn();
    render(
      <MessageView
        message={messageFixture({
          kind: "outbox",
          delivery_status_label: "投递结果未知",
          outbox: {
            id: "unknown-outbox",
            status: "delivery_unknown",
            attempts: 2,
          },
        })}
        onClose={vi.fn()}
        onRetryDelivery={vi.fn()}
        onResolveDeliveryUnknown={onResolveDeliveryUnknown}
        canRetryDelivery
      />,
    );

    expect(screen.queryByRole("button", { name: "重试发送" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "确认已投递" }));
    await user.click(screen.getByRole("button", { name: "仍要重试" }));

    expect(onResolveDeliveryUnknown.mock.calls).toEqual([
      ["confirm_delivered"],
      ["retry_once"],
    ]);
  });

  it.each([
    ["blank id", { id: "  ", attempts: 1 }],
    ["numeric id", { id: 42, attempts: 1 }],
    ["string attempts", { id: "unknown-outbox", attempts: "1" }],
    ["unsafe attempts", { id: "unknown-outbox", attempts: 0x100000000 }],
  ])("does not expose delivery-unknown decisions for %s", (_, malformed) => {
    render(
      <MessageView
        message={messageFixture({
          kind: "outbox",
          delivery_status_label: "投递结果未知",
          outbox: {
            ...malformed,
            status: "delivery_unknown",
          },
        })}
        onClose={vi.fn()}
        onResolveDeliveryUnknown={vi.fn()}
        canRetryDelivery
      />,
    );

    expect(screen.queryByRole("button", { name: "确认已投递" })).toBeNull();
    expect(screen.queryByRole("button", { name: "仍要重试" })).toBeNull();
    expect(screen.getByText("发件队列记录不完整，请刷新后再处理。")).toBeTruthy();
  });

  it("does not render click-looking actions when callbacks are absent", () => {
    render(<MessageView message={messageFixture()} onClose={vi.fn()} />);

    expect(screen.queryByRole("button", { name: "归档" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "移到垃圾箱" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "标记为未读" }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "回复" })).toBeNull();
    expect(screen.queryByRole("button", { name: "转发" })).toBeNull();
  });

  it("keeps unread synchronization out of the reader while retaining actionable Archive and Trash feedback", async () => {
    const user = userEvent.setup();
    const retryMove = vi.fn();

    render(
      <MessageView
        message={messageFixture()}
        onClose={vi.fn()}
        onArchive={vi.fn()}
        archiveState={{ status: "pending" }}
        onMoveToTrash={retryMove}
        moveToTrashState={{
          status: "error",
          message: "网络连接中断",
          retryable: true,
        }}
        onMarkUnread={vi.fn()}
        markUnreadState={{
          status: "outcome_unknown",
          message: "正在核对服务器状态",
        }}
      />,
    );

    const pendingArchive = screen.getByRole("button", {
      name: "正在归档…",
    });
    expect(pendingArchive.disabled).toBe(true);
    expect(pendingArchive.getAttribute("aria-busy")).toBe("true");
    expect(
      screen.queryByText("标记为未读结果待确认：正在核对服务器状态"),
    ).toBeNull();
    expect(
      screen.getByRole("button", { name: "标记为未读" }).disabled,
    ).toBe(false);

    const retryButton = screen.getByRole("button", {
      name: "重试移到垃圾箱：网络连接中断",
    });
    await user.click(retryButton);
    expect(retryMove).toHaveBeenCalledOnce();
  });

  it("hides an unavailable capability and explains why", () => {
    render(
      <MessageView
        message={messageFixture()}
        onClose={vi.fn()}
        onArchive={vi.fn()}
        archiveState={{
          status: "unavailable",
          message: "账户没有可用的归档邮箱",
        }}
      />,
    );

    expect(screen.queryByRole("button", { name: /归档/ })).toBeNull();
    expect(
      screen.getByText("归档不可用：账户没有可用的归档邮箱"),
    ).toBeTruthy();
  });

  it("does not let a stale mark-read state disable the mark-unread action", () => {
    render(
      <MessageView
        message={messageFixture()}
        onClose={vi.fn()}
        onMarkUnread={vi.fn()}
        markUnreadState={{
          status: "needs_attention",
          message: "邮箱标识已变化，请先重新同步",
        }}
      />,
    );

    const button = screen.getByRole("button", { name: "标记为未读" });
    expect(button.disabled).toBe(false);
    expect(
      screen.queryByText(
        "标记为未读需要处理：邮箱标识已变化，请先重新同步",
      ),
    ).toBeNull();
  });
});

describe("MessageView received attachments", () => {
  it("uses authoritative metadata, safe text, exact part size, and the opaque save id", async () => {
    const user = userEvent.setup();
    const onSaveAttachment = vi.fn();
    const attachment = {
      id: "part-opaque-7",
      original_name: "../../secret.pdf",
      safe_display_name: "<img src=x onerror=alert(1)>.png",
      mime_type: "image/png",
      size_bytes: 2048,
      disposition: "attachment",
    };
    const { container } = render(
      <MessageView
        message={messageFixture({
          size_bytes: 9999999,
          attachments: [attachment],
          attachment_names: ["legacy.pdf"],
        })}
        onClose={vi.fn()}
        onSaveAttachment={onSaveAttachment}
      />,
    );

    expect(
      screen.getByText("<img src=x onerror=alert(1)>.png"),
    ).toBeTruthy();
    expect(screen.getByText("image/png · 2 KB · 2,048 字节")).toBeTruthy();
    expect(screen.queryByText("legacy.pdf")).toBeNull();
    expect(container.querySelector(".attachment-card img")).toBeNull();

    await user.click(
      screen.getByRole("button", {
        name: "保存附件 <img src=x onerror=alert(1)>.png",
      }),
    );
    expect(onSaveAttachment).toHaveBeenCalledWith("part-opaque-7", attachment);
  });

  it("renders saving, saved, canceled, and retryable error independently", async () => {
    const user = userEvent.setup();
    const onSaveAttachment = vi.fn();
    const attachments = [
      {
        id: "saving",
        safe_display_name: "saving.bin",
        mime_type: "application/octet-stream",
        size_bytes: 5,
        disposition: "attachment",
      },
      {
        id: "saved",
        safe_display_name: "saved.pdf",
        mime_type: "application/pdf",
        size_bytes: 7,
        disposition: "attachment",
      },
      {
        id: "canceled",
        safe_display_name: "canceled.zip",
        mime_type: "application/zip",
        size_bytes: 9,
        disposition: "attachment",
      },
      {
        id: "error",
        safe_display_name: "retry.txt",
        mime_type: "text/plain",
        size_bytes: 11,
        disposition: "attachment",
      },
    ];

    render(
      <MessageView
        message={messageFixture({ attachments })}
        onClose={vi.fn()}
        onSaveAttachment={onSaveAttachment}
        attachmentSaveStates={{
          saving: { status: "saving" },
          saved: { status: "saved", file_name: "saved (1).pdf" },
          canceled: { status: "canceled" },
          error: {
            status: "error",
            message: "磁盘空间不足",
            retryable: true,
          },
        }}
      />,
    );

    expect(
      screen.getByRole("group", { name: "正在保存附件 saving.bin" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("group", {
        name: "附件 saved.pdf 已保存为 saved (1).pdf",
      }),
    ).toBeTruthy();
    expect(screen.getByText("已取消，可重新保存")).toBeTruthy();
    expect(screen.getByText("保存失败：磁盘空间不足，可重试")).toBeTruthy();

    await user.click(
      screen.getByRole("button", { name: "重新保存附件 canceled.zip" }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "重试保存附件 retry.txt：磁盘空间不足",
      }),
    );
    expect(onSaveAttachment.mock.calls.map(([id]) => id)).toEqual([
      "canceled",
      "error",
    ]);
  });

  it("shows legacy attachment names as non-interactive compatibility information", () => {
    render(
      <MessageView
        message={messageFixture({
          attachment_names: ["legacy.pdf"],
          size_bytes: 100000,
        })}
        onClose={vi.fn()}
      />,
    );

    expect(
      screen.getByText("附件详情尚未加载。重新同步或加载完整邮件后才能保存。"),
    ).toBeTruthy();
    expect(
      screen.getByRole("group", {
        name: "附件 legacy.pdf，详情尚未加载",
      }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: /legacy\.pdf/ }),
    ).toBeNull();
    expect(screen.getByText("类型和大小未知")).toBeTruthy();
  });

  it("marks an authoritative attachment as disabled when no save callback exists", () => {
    render(
      <MessageView
        message={messageFixture({
          attachments: [
            {
              id: "read-only-part",
              safe_display_name: "只读附件.txt",
              mime_type: "text/plain",
              size_bytes: 12,
              disposition: "attachment",
            },
          ],
        })}
        onClose={vi.fn()}
      />,
    );

    const attachment = screen.getByRole("group", {
      name: "附件 只读附件.txt，当前无法保存",
    });
    expect(attachment.dataset.state).toBe("disabled");
    expect(attachment.tagName).toBe("DIV");
  });
});

describe("MessageView forward preparation", () => {
  it("requires the preparation callback and ignores the unsafe legacy callback", async () => {
    const user = userEvent.setup();
    const onPrepareForward = vi.fn();
    const { rerender } = render(
      <MessageView
        message={messageFixture({ preview: "不能用于转发的列表摘要" })}
        onClose={vi.fn()}
        onPrepareForward={onPrepareForward}
      />,
    );

    await user.click(screen.getByRole("button", { name: "转发" }));
    expect(onPrepareForward).toHaveBeenCalledOnce();
    expect(onPrepareForward).toHaveBeenCalledWith();

    const legacyForward = vi.fn();
    rerender(
      <MessageView
        message={messageFixture()}
        onClose={vi.fn()}
        onForward={legacyForward}
      />,
    );
    expect(screen.queryByRole("button", { name: "转发" })).toBeNull();
    expect(legacyForward).not.toHaveBeenCalled();
  });

  it("keeps bounded loading and makes failed preparation silently retryable", async () => {
    const user = userEvent.setup();
    const onPrepareForward = vi.fn();
    const { rerender } = render(
      <MessageView
        message={messageFixture()}
        onClose={vi.fn()}
        onPrepareForward={onPrepareForward}
        forwardState={{ status: "loading" }}
      />,
    );

    const loadingButton = screen.getByRole("button", {
      name: "正在准备转发…",
    });
    expect(loadingButton.disabled).toBe(true);
    expect(loadingButton.getAttribute("aria-busy")).toBe("true");

    rerender(
      <MessageView
        message={messageFixture()}
        onClose={vi.fn()}
        onPrepareForward={onPrepareForward}
        forwardState={{
          status: "error",
          message: "一个附件无法安全准备",
          retry_without_attachments_allowed: true,
        }}
      />,
    );

    expect(screen.queryByText("一个附件无法安全准备")).toBeNull();
    expect(screen.queryByText("转发准备失败")).toBeNull();
    expect(screen.queryByRole("button", { name: "无附件转发" })).toBeNull();
    await user.click(
      screen.getByRole("button", { name: "重试准备转发" }),
    );
    expect(onPrepareForward).toHaveBeenCalledOnce();
  });
});

describe("MessageView recipient details", () => {
  it("shows From/To/Cc/Bcc with real addresses, supports Escape, and restores toggle focus", async () => {
    const user = userEvent.setup();
    render(
      <MessageView
        message={messageFixture({
          sender: {
            name: "邮件头发件人",
            email:
              "sender-with-a-very-long-local-part-for-reader@example-subdomain.example.com",
          },
          to: [
            { name: "收件人甲", email: "to-a@example.com" },
            { name: null, email: "to-b@example.com" },
          ],
          cc: [{ name: "抄送乙", email: "cc@example.com" }],
          bcc: [{ name: "密送丙", email: "bcc@example.com" }],
        })}
        senderDisplayName="本地备注发件人"
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText("本地备注发件人")).toBeTruthy();
    expect(
      screen.getByText(
        "sender-with-a-very-long-local-part-for-reader@example-subdomain.example.com",
      ),
    ).toBeTruthy();

    const toggle = screen.getByRole("button", { name: "查看收件人" });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    await user.click(toggle);

    const region = screen.getByRole("region", { name: "收件人详情" });
    expect(region).toBeTruthy();
    expect(document.activeElement).toBe(region);
    expect(screen.getByText("发件人")).toBeTruthy();
    expect(screen.getAllByText("本地备注发件人")).toHaveLength(2);
    expect(
      screen.getAllByText(
        "sender-with-a-very-long-local-part-for-reader@example-subdomain.example.com",
      ),
    ).toHaveLength(2);
    expect(screen.getByText("收件人甲")).toBeTruthy();
    expect(screen.getByText("to-a@example.com")).toBeTruthy();
    expect(screen.getByText("to-b@example.com")).toBeTruthy();
    expect(screen.getByText("抄送乙")).toBeTruthy();
    expect(screen.getByText("cc@example.com")).toBeTruthy();
    expect(screen.getByText("密送丙")).toBeTruthy();
    expect(screen.getByText("bcc@example.com")).toBeTruthy();

    fireEvent.keyDown(region, { key: "Escape" });
    expect(
      screen.queryByRole("region", { name: "收件人详情" }),
    ).toBeNull();
    expect(document.activeElement).toBe(toggle);
  });

  it("supports a standalone from identity and omits an empty Bcc group", async () => {
    const user = userEvent.setup();
    render(
      <MessageView
        message={messageFixture({
          sender: undefined,
          from: { name: "仅 From 发件人", email: "from-only@example.com" },
          bcc: [],
        })}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText("仅 From 发件人")).toBeTruthy();
    expect(screen.getByText("from-only@example.com")).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "查看收件人" }));

    expect(screen.getByText("发件人")).toBeTruthy();
    expect(screen.queryByText("密送")).toBeNull();
  });

  it("uses exact Outbox recipient groups and never infers them from flat recipients", async () => {
    const user = userEvent.setup();
    render(
      <MessageView
        message={messageFixture({
          kind: "outbox",
          sender: { name: "当前账户", email: "me@example.com" },
          to: ["flat-recipient-must-not-appear@example.com"],
          cc: ["flat-cc-must-not-appear@example.com"],
          bcc: ["flat-bcc-must-not-appear@example.com"],
          recipient_groups: {
            to: ["exact-to@example.com"],
            cc: ["exact-cc@example.com"],
            bcc: ["exact-bcc@example.com"],
          },
        })}
        onClose={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "查看收件人" }));

    expect(screen.getByText("exact-to@example.com")).toBeTruthy();
    expect(screen.getByText("exact-cc@example.com")).toBeTruthy();
    expect(screen.getByText("exact-bcc@example.com")).toBeTruthy();
    expect(
      screen.queryByText("flat-recipient-must-not-appear@example.com"),
    ).toBeNull();
    expect(
      screen.queryByText("flat-cc-must-not-appear@example.com"),
    ).toBeNull();
    expect(
      screen.queryByText("flat-bcc-must-not-appear@example.com"),
    ).toBeNull();
    expect(
      screen.queryByText("旧版邮件收件人分组不可用"),
    ).toBeNull();
  });

  it("labels legacy Outbox grouping as unavailable without reconstructing flat recipients", async () => {
    const user = userEvent.setup();
    render(
      <MessageView
        message={messageFixture({
          kind: "outbox",
          sender: { name: "当前账户", email: "me@example.com" },
          to: ["flat-recipient-must-not-appear@example.com"],
          cc: ["flat-cc-must-not-appear@example.com"],
          bcc: ["flat-bcc-must-not-appear@example.com"],
          recipient_groups: null,
        })}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText("旧版邮件收件人分组不可用")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "查看收件人" }));

    expect(screen.getByText("旧版邮件收件人分组不可用")).toBeTruthy();
    expect(
      screen.queryByText("flat-recipient-must-not-appear@example.com"),
    ).toBeNull();
    expect(
      screen.queryByText("flat-cc-must-not-appear@example.com"),
    ).toBeNull();
    expect(
      screen.queryByText("flat-bcc-must-not-appear@example.com"),
    ).toBeNull();
    expect(screen.queryByText("收件人")).toBeNull();
    expect(screen.queryByText("抄送")).toBeNull();
    expect(screen.queryByText("密送")).toBeNull();
  });
});
