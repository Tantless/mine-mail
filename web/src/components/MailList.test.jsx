import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MailList } from "./MailList.jsx";

const firstMessage = {
  id: "mail-inbox-11",
  uid: 11,
  mailbox: "INBOX",
  sender: { name: "林然", email: "lin@example.com" },
  subject: "项目进度",
  preview: "这是已同步的摘要",
  sent_at: "2026-07-28T08:00:00.000Z",
  flags: [],
};

const secondMessage = {
  id: "mail-inbox-10",
  uid: 10,
  mailbox: "INBOX",
  sender: { name: "陈冬", email: "chen@example.com" },
  subject: "会议安排",
  preview: "明天下午见",
  sent_at: "2026-07-27T08:00:00.000Z",
  flags: ["\\Seen", "\\Flagged"],
};

function renderMailList(overrides = {}) {
  const baseProps = {
    folderRole: "inbox",
    messages: [],
    selectedMessageId: null,
    onSelect: vi.fn(),
    onToggleStar: vi.fn(),
    query: "",
    onQueryChange: vi.fn(),
    filter: "all",
    onFilterChange: vi.fn(),
    onSync: vi.fn(),
    syncState: "idle",
    loadState: { phase: "ready", completed: 0, total: null },
    canSync: true,
  };
  const view = render(<MailList {...baseProps} {...overrides} />);
  return {
    ...view,
    rerenderMailList(nextOverrides = {}) {
      view.rerender(
        <MailList
          {...baseProps}
          {...overrides}
          {...nextOverrides}
        />,
      );
    },
  };
}

describe("MailList folder contracts", () => {
  afterEach(cleanup);

  it.each([
    ["inbox", "收件箱", "INBOX", "同步收件箱", ["全部", "未读", "收藏"]],
    ["starred", "已收藏", "STARRED", "同步已收藏邮件", ["全部", "未读"]],
    ["sent", "已发送", "SENT", "同步已发送", ["全部", "收藏"]],
    ["drafts", "草稿", "DRAFTS", "同步草稿", []],
    ["outbox", "发件队列", "OUTBOX", "刷新发件队列", []],
    ["archive", "归档", "ARCHIVE", "同步归档", ["全部", "未读", "收藏"]],
    ["trash", "垃圾箱", "TRASH", "同步垃圾箱", ["全部", "未读"]],
  ])(
    "uses %s-specific heading, filters, and sync copy",
    (folderRole, title, eyebrow, syncLabel, filters) => {
      const { container } = renderMailList({ folderRole });

      expect(screen.getByRole("heading", { name: title })).toBeTruthy();
      expect(screen.getByText(eyebrow)).toBeTruthy();
      expect(screen.getByRole("button", { name: syncLabel })).toBeTruthy();
      expect(
        Array.from(container.querySelectorAll(".mail-tab")).map(
          (tab) => tab.textContent,
        ),
      ).toEqual(filters);
      expect(container.querySelector(".empty-list")).toBeNull();
    },
  );

  it("infers the folder role from the legacy folderLabel prop", () => {
    const { container } = renderMailList({
      folderRole: undefined,
      folderLabel: "草稿",
    });

    expect(screen.getByText("DRAFTS")).toBeTruthy();
    expect(container.querySelector(".mail-tab")).toBeNull();
  });

  it.each(["drafts", "outbox", "trash"])(
    "does not expose meaningless star controls in %s",
    (folderRole) => {
      renderMailList({
        folderRole,
        messages: [{ ...firstMessage, kind: folderRole.slice(0, -1) }],
      });

      expect(screen.queryByRole("button", { name: /收藏：/ })).toBeNull();
    },
  );

  it.each([
    ["inbox", "正在同步收件箱"],
    ["starred", "正在同步已收藏邮件"],
    ["sent", "正在同步已发送"],
    ["drafts", "正在同步草稿"],
    ["outbox", "正在刷新发件队列"],
    ["archive", "正在同步归档"],
    ["trash", "正在同步垃圾箱"],
  ])("names active %s synchronization precisely", (folderRole, label) => {
    renderMailList({ folderRole, syncState: "syncing" });

    const sync = screen.getByRole("button", { name: label });
    expect(sync.disabled).toBe(true);
    expect(sync.getAttribute("aria-busy")).toBe("true");
  });
});

describe("MailList controlled controls", () => {
  afterEach(cleanup);

  it("states the bounded search scope while retaining the established accessible name", async () => {
    const user = userEvent.setup();
    const onQueryChange = vi.fn();
    renderMailList({ onQueryChange });

    const search = screen.getByLabelText("搜索邮件");
    expect(search.placeholder).toBe("搜索已同步邮件");
    expect(search.getAttribute("aria-description")).toBe(
      "范围：搜索已同步邮件",
    );

    await user.type(search, "项目");
    expect(onQueryChange).toHaveBeenCalled();
  });

  it("hides the funnel by default and only exposes a working or accurately disabled control", async () => {
    const user = userEvent.setup();
    const onOpenFilters = vi.fn();
    const { rerenderMailList } = renderMailList();

    expect(screen.queryByRole("button", { name: "筛选邮件" })).toBeNull();

    rerenderMailList({ onOpenFilters });
    await user.click(screen.getByRole("button", { name: "筛选邮件" }));
    expect(onOpenFilters).toHaveBeenCalledOnce();

    rerenderMailList({
      onOpenFilters: null,
      filterDisabledReason: "此文件夹没有更多筛选条件",
    });
    expect(
      screen.getByRole("button", {
        name: "筛选邮件不可用：此文件夹没有更多筛选条件",
      }).disabled,
    ).toBe(true);
  });

  it("renders no dead controls when their callbacks are absent", () => {
    renderMailList({
      messages: [firstMessage],
      onFilterChange: null,
      onOpenFilters: null,
      onQueryChange: null,
      onSelect: null,
      onSync: null,
      onToggleStar: null,
      onLoadMore: null,
    });

    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByRole("list", { name: "邮件" })).toBeTruthy();
  });

  it("does not mark every row selected when legacy identity fields are absent", () => {
    const { container } = renderMailList({
      messages: [
        { ...firstMessage, id: undefined, uid: undefined },
        { ...secondMessage, id: undefined, uid: undefined },
      ],
      selectedMessageId: null,
      selectedMessage: null,
    });

    expect(
      Array.from(container.querySelectorAll(".mail-row")).every(
        (row) => row.dataset.selected === "false",
      ),
    ).toBe(true);
  });

  it("keeps an unavailable sync action disabled with a reason", () => {
    renderMailList({
      canSync: false,
      syncDisabledReason: "账户当前离线",
    });

    const sync = screen.getByRole("button", { name: "同步收件箱" });
    expect(sync.disabled).toBe(true);
  });
});

describe("MailList state distinctions", () => {
  afterEach(cleanup);

  it("keeps empty, loading, failure, search, and capability states visually quiet", () => {
    const { container, rerenderMailList } = renderMailList({
      loadState: { phase: "loading", completed: 0, total: null },
    });
    expect(container.querySelector(".empty-list")).toBeNull();
    expect(container.querySelector(".mail-loading-state")).toBeNull();

    rerenderMailList({
      query: "不存在",
      loadState: { phase: "error", completed: 0, total: null },
    });
    expect(screen.queryByText("没有匹配的已同步邮件")).toBeNull();
    expect(screen.queryByText("部分邮件暂时没有加载完成")).toBeNull();

    rerenderMailList({
      folderRole: "archive",
      query: "",
      mailboxCapability: {
        role: "archive",
        status: "needs_creation_confirmation",
        retryable: true,
      },
    });
    expect(screen.queryByText("尚未设置归档文件夹")).toBeNull();
    expect(container.querySelector(".empty-list")).toBeNull();
  });
});

describe("MailList pagination and semantics", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("loads automatically near the bottom and keeps completion feedback for two seconds", () => {
    vi.useFakeTimers();
    const onLoadMore = vi.fn();
    const { container, rerenderMailList } = renderMailList({
      messages: [firstMessage],
      onLoadMore,
      loadMoreState: "idle",
    });
    const surface = container.querySelector(".message-list");
    Object.defineProperties(surface, {
      scrollHeight: { configurable: true, value: 800 },
      clientHeight: { configurable: true, value: 400 },
      scrollTop: { configurable: true, value: 350 },
    });

    fireEvent.scroll(surface);
    expect(onLoadMore).toHaveBeenCalledOnce();
    fireEvent.scroll(surface);
    expect(onLoadMore).toHaveBeenCalledOnce();
    expect(
      screen.queryByRole("button", { name: /加载更早邮件/ }),
    ).toBeNull();

    rerenderMailList({ loadMoreState: "loading" });
    expect(screen.getByText("正在加载…")).toBeTruthy();
    rerenderMailList({
      messages: [firstMessage, secondMessage],
      loadMoreState: "idle",
    });
    expect(screen.getByText("已加载 1 封")).toBeTruthy();

    act(() => {
      vi.advanceTimersByTime(2_000);
    });
    expect(screen.queryByText("已加载 1 封")).toBeNull();
    vi.useRealTimers();
  });

  it("shows a compact two-second failure only after a loading attempt", () => {
    vi.useFakeTimers();
    const { rerenderMailList } = renderMailList({
      messages: [firstMessage],
      loadMoreState: "idle",
    });

    rerenderMailList({ loadMoreState: "loading" });
    expect(screen.getByText("正在加载…")).toBeTruthy();
    rerenderMailList({ loadMoreState: "retry" });
    const failure = screen.getByRole("alert");
    expect(failure.textContent).toBe("加载失败");
    expect(failure.className).toBe("mail-pagination-notice");

    act(() => {
      vi.advanceTimersByTime(2_000);
    });
    expect(screen.queryByText("加载失败")).toBeNull();
    vi.useRealTimers();
  });

  it("does not render persistent offline, unavailable, or confirmed-end copy", () => {
    const { rerenderMailList } = renderMailList({
      messages: [firstMessage],
      loadMoreState: "offline",
    });
    expect(screen.queryByText(/连接网络|已显示全部|加载更早/)).toBeNull();

    rerenderMailList({ loadMoreState: "unavailable" });
    expect(screen.queryByText(/无法提供|已显示全部|加载更早/)).toBeNull();

    rerenderMailList({ loadMoreState: "complete" });
    expect(screen.queryByText(/已显示全部|加载更早/)).toBeNull();
  });

  it("uses a normal list with native row buttons instead of a partial listbox", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    renderMailList({
      messages: [firstMessage, secondMessage],
      onSelect,
    });

    expect(screen.getByRole("list", { name: "邮件" })).toBeTruthy();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(screen.queryByRole("option")).toBeNull();

    const open = screen.getByRole("button", {
      name: "打开邮件：林然，项目进度",
    });
    open.focus();
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith(firstMessage);
  });

  it.each([
    ["numeric", 9007199254740993],
    ["empty", "   "],
  ])(
    "keeps a %s message id non-interactive instead of falling back to legacy identity",
    async (_label, id) => {
      const user = userEvent.setup();
      const onSelect = vi.fn();
      const onToggleStar = vi.fn();
      const invalidMessage = {
        ...firstMessage,
        id,
        uid: 99,
        mailbox: "INBOX",
      };
      renderMailList({
        messages: [invalidMessage],
        onSelect,
        onToggleStar,
      });

      const row = screen.getByRole("listitem");
      expect(
        screen.queryByRole("button", {
          name: "打开邮件：林然，项目进度",
        }),
      ).toBeNull();
      expect(
        screen.queryByRole("button", {
          name: "添加收藏：项目进度",
        }),
      ).toBeNull();

      await user.click(row);
      expect(onSelect).not.toHaveBeenCalled();
      expect(onToggleStar).not.toHaveBeenCalled();
    },
  );

  it("preserves native list semantics when only the legacy folder label is supplied", () => {
    renderMailList({
      folderRole: undefined,
      folderLabel: "收件箱",
      messages: [firstMessage],
    });

    expect(screen.getByRole("list", { name: "邮件" })).toBeTruthy();
    expect(screen.getByRole("listitem")).toBeTruthy();
    expect(screen.queryByRole("option")).toBeNull();
  });

  it("preserves the selected stable key when an older page is appended", () => {
    const { container, rerenderMailList } = renderMailList({
      messages: [firstMessage, secondMessage],
      selectedMessage: secondMessage,
    });
    const selectedBefore = container.querySelector(
      '[data-navigation-key="message:mail-inbox-10"]',
    );
    expect(selectedBefore?.dataset.selected).toBe("true");

    const older = {
      ...firstMessage,
      id: "mail-inbox-09",
      subject: "更早的邮件",
      sent_at: "2026-07-26T08:00:00.000Z",
    };
    rerenderMailList({
      messages: [firstMessage, secondMessage, older],
      selectedMessage: secondMessage,
    });

    const selectedAfter = container.querySelector(
      '[data-navigation-key="message:mail-inbox-10"]',
    );
    expect(selectedAfter).toBe(selectedBefore);
    expect(selectedAfter?.dataset.selected).toBe("true");
    expect(
      screen
        .getByRole("button", { name: "打开邮件：陈冬，会议安排" })
        .getAttribute("aria-current"),
    ).toBe("true");
  });
});
