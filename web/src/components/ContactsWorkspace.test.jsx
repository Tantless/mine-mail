import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ContactsWorkspace } from "./ContactsWorkspace.jsx";

const contact = {
  email: "friend@example.com",
  displayName: "小林",
  isFavorite: false,
  messageCount: 2,
  lastMessageAt: "2026-07-20T08:30:00Z",
  lastSubject: "周末见",
  avatarSrc: null,
};

const messages = [
  {
    uid: 42,
    mailbox: "INBOX",
    kind: "inbox",
    subject: "周末见",
    preview: "我们周六下午见吧。",
    sent_at: "2026-07-20T08:30:00Z",
  },
  {
    uid: 41,
    mailbox: "Sent",
    kind: "sent",
    subject: "Re: 地址",
    preview: "好的，我把地址发给你。",
    sent_at: "2026-07-19T08:30:00Z",
  },
];

function renderWorkspace(overrides = {}) {
  const callbacks = {
    onSearchChange: vi.fn(),
    onFilterChange: vi.fn(),
    onSelectContact: vi.fn(),
    onToggleFavorite: vi.fn(),
    onCompose: vi.fn(),
    onOpenMessage: vi.fn(),
    onSaveRemark: vi.fn().mockResolvedValue(undefined),
  };

  render(
    <ContactsWorkspace
      contacts={[contact]}
      selectedContact={contact}
      messages={messages}
      query=""
      filter="all"
      {...callbacks}
      {...overrides}
    />,
  );
  return callbacks;
}

describe("ContactsWorkspace", () => {
  afterEach(cleanup);

  it("renders controlled search and filters and selects a contact", async () => {
    const user = userEvent.setup();
    const callbacks = renderWorkspace();

    await user.type(screen.getByRole("textbox", { name: "搜索联系人" }), "lin");
    expect(callbacks.onSearchChange).toHaveBeenLastCalledWith("n");

    await user.click(screen.getByRole("tab", { name: "收藏" }));
    expect(callbacks.onFilterChange).toHaveBeenCalledWith("favorite");
    expect(screen.queryByRole("tab", { name: "已保存" })).toBeNull();

    await user.click(screen.getByRole("button", { name: "查看联系人 小林" }));
    expect(callbacks.onSelectContact).toHaveBeenCalledWith(contact);
  });

  it("keeps row favorite independent and exposes detail actions", async () => {
    const user = userEvent.setup();
    const callbacks = renderWorkspace();

    const favoriteButtons = screen.getAllByRole("button", { name: "收藏 小林" });
    await user.click(favoriteButtons[0]);
    expect(callbacks.onToggleFavorite).toHaveBeenCalledWith(contact);
    expect(callbacks.onSelectContact).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "写信" }));
    expect(callbacks.onCompose).toHaveBeenCalledWith(contact);
    expect(screen.queryByRole("button", { name: "保存联系人" })).toBeNull();
    expect(screen.queryByRole("button", { name: "编辑名称" })).toBeNull();
    expect(screen.queryByRole("button", { name: "移除联系人" })).toBeNull();
  });

  it("opens a correspondence message", async () => {
    const user = userEvent.setup();
    const callbacks = renderWorkspace();

    await user.click(screen.getByRole("button", { name: "打开邮件：周末见" }));
    expect(callbacks.onOpenMessage).toHaveBeenCalledWith(messages[0]);
  });

  it("shows the original name beneath a remark and saves remark edits", async () => {
    const user = userEvent.setup();
    const remarkedContact = {
      ...contact,
      displayName: "林老师",
      originalName: "小林",
      remark: "林老师",
    };
    const callbacks = renderWorkspace({
      contacts: [remarkedContact],
      selectedContact: remarkedContact,
    });

    expect(screen.getByRole("heading", { name: "林老师" })).toBeTruthy();
    expect(screen.getByText("原名：小林")).toBeTruthy();
    expect(screen.getByRole("button", { name: "查看联系人 林老师" })).toBeTruthy();

    const input = screen.getByRole("textbox", { name: "联系人备注名" });
    await user.clear(input);
    await user.type(input, "  林同学  ");
    await user.click(screen.getByRole("button", { name: "保存备注" }));

    expect(callbacks.onSaveRemark).toHaveBeenCalledWith(remarkedContact, "林同学");
  });

  it("marks favorite rows with the pinned surface hook", () => {
    const favorite = { ...contact, isFavorite: true };
    renderWorkspace({ selectedContact: favorite, contacts: [favorite] });

    const list = screen.getByRole("list", { name: "联系人" });
    expect(within(list).getByRole("listitem").getAttribute("data-favorite")).toBe("true");
  });

  it("orders correspondence newest first without mutating the input", () => {
    const reversed = [...messages].reverse();
    renderWorkspace({ messages: reversed });

    expect(
      screen.getAllByRole("button", { name: /打开邮件/ })[0].getAttribute("aria-label"),
    ).toBe("打开邮件：周末见");
    expect(reversed[0]).toBe(messages[1]);
  });

  it.each([
    [{ isLoading: true, contacts: [] }, "正在加载联系人"],
    [{ error: "数据库暂不可用", contacts: [] }, "联系人加载失败"],
    [{ contacts: [], selectedContact: null, messages: [] }, "还没有联系人"],
    [{ selectedContact: null, messages: [] }, "选择一个联系人"],
    [{ messages: [] }, "还没有往来邮件"],
    [{ isMessagesLoading: true, messages: [] }, "正在加载往来邮件"],
    [{ messagesError: "读取失败", messages: [] }, "往来邮件加载失败"],
  ])("renders state %o", (props, expectedText) => {
    renderWorkspace(props);
    expect(screen.getByText(expectedText, { exact: false })).toBeTruthy();
  });

  it("replaces only the detail column when readerContent is provided", () => {
    renderWorkspace({ readerContent: <section aria-label="复用邮件阅读器">邮件正文</section> });

    expect(screen.getByLabelText("通讯录联系人列表")).toBeTruthy();
    expect(screen.getByLabelText("复用邮件阅读器")).toBeTruthy();
    expect(screen.queryByLabelText("小林 的联系人详情")).toBeNull();
  });
});
