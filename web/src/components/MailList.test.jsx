import { cleanup, render, screen } from "@testing-library/react";
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
    ["inbox", "收件箱", "INBOX", "同步收件箱", ["全部", "未读", "收藏"], "收件箱里还没有邮件"],
    ["starred", "已收藏", "STARRED", "同步已收藏邮件", ["全部", "未读"], "还没有收藏邮件"],
    ["sent", "已发送", "SENT", "同步已发送", ["全部", "收藏"], "还没有已发送邮件"],
    ["drafts", "草稿", "DRAFTS", "同步草稿", [], "还没有草稿"],
    ["outbox", "发件队列", "OUTBOX", "刷新发件队列", [], "发件队列为空"],
    ["archive", "归档", "ARCHIVE", "同步归档", ["全部", "未读", "收藏"], "归档里还没有邮件"],
    ["trash", "垃圾箱", "TRASH", "同步垃圾箱", ["全部", "未读"], "垃圾箱里还没有邮件"],
  ])(
    "uses %s-specific heading, filters, sync copy, and true empty copy",
    (folderRole, title, eyebrow, syncLabel, filters, emptyTitle) => {
      const { container } = renderMailList({ folderRole });

      expect(screen.getByRole("heading", { name: title })).toBeTruthy();
      expect(screen.getByText(eyebrow)).toBeTruthy();
      expect(screen.getByRole("button", { name: syncLabel })).toBeTruthy();
      expect(
        Array.from(container.querySelectorAll(".mail-tab")).map(
          (tab) => tab.textContent,
        ),
      ).toEqual(filters);
      expect(screen.getByText(emptyTitle)).toBeTruthy();
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

  it.each([
    ["search_empty", "没有匹配的已同步邮件"],
    ["filter_empty", "当前筛选下没有邮件"],
    ["initial_sync", "正在同步收件箱…"],
    ["not_synced", "收件箱尚未同步"],
    ["offline_history_exhausted", "已显示全部本地邮件"],
    ["loading_older", "正在加载更早邮件…"],
    ["retryable_failure", "部分邮件暂时没有加载完成"],
    ["confirmed_end", "已确认没有更多邮件"],
    ["true_empty", "收件箱里还没有邮件"],
  ])("renders the %s state without reusing generic search copy", (emptyState, copy) => {
    renderMailList({ emptyState });
    expect(screen.getByText(copy)).toBeTruthy();
  });

  it("derives search and filter empty states from the controlled values", () => {
    const { rerenderMailList } = renderMailList({ query: "不存在" });
    expect(screen.getByText("没有匹配的已同步邮件")).toBeTruthy();

    rerenderMailList({ query: "", filter: "unread" });
    expect(screen.getByText("当前筛选下没有邮件")).toBeTruthy();
  });

  it.each([
    ["inbox", "收件箱尚未同步", "使用“同步收件箱”获取邮件"],
    ["starred", "收藏来源尚未同步", "使用“同步已收藏邮件”获取邮件"],
    ["sent", "已发送尚未同步", "使用“同步已发送”获取邮件"],
    ["drafts", "草稿尚未同步", "使用“同步草稿”读取已保存草稿"],
    ["outbox", "发件队列尚未读取", "使用“刷新发件队列”读取待处理邮件"],
    ["archive", "归档尚未同步", "使用“同步归档”获取邮件"],
    ["trash", "垃圾箱尚未同步", "使用“同步垃圾箱”获取邮件"],
  ])(
    "uses an actionable not-synced state for %s",
    (folderRole, title, detail) => {
      renderMailList({ folderRole, isInitialized: false });
      expect(screen.getByText(title)).toBeTruthy();
      expect(screen.getByText(detail)).toBeTruthy();
    },
  );

  it("does not let search or filter copy hide loading, failure, or an unsynchronized source", () => {
    const { rerenderMailList } = renderMailList({
      query: "项目",
      loadState: { phase: "loading", completed: 0, total: null },
      isInitialized: false,
    });
    expect(screen.getByText("正在读取收件箱本地邮件…")).toBeTruthy();
    expect(screen.queryByText("没有匹配的已同步邮件")).toBeNull();

    rerenderMailList({
      query: "项目",
      loadState: { phase: "error", completed: 0, total: null },
      isInitialized: false,
    });
    expect(screen.getByText("部分邮件暂时没有加载完成")).toBeTruthy();

    rerenderMailList({
      query: "项目",
      filter: "unread",
      loadState: { phase: "ready", completed: 0, total: null },
      isInitialized: false,
    });
    expect(screen.getByText("收件箱尚未同步")).toBeTruthy();
    expect(screen.queryByText("当前筛选下没有邮件")).toBeNull();
  });

  it("only recommends controls that are actually rendered and usable", () => {
    const { rerenderMailList } = renderMailList({
      query: "不存在",
      onLoadMore: null,
      onSync: null,
    });
    expect(screen.getByText("换个关键词后重试")).toBeTruthy();
    expect(screen.queryByText(/加载更多|同步更多/)).toBeNull();

    rerenderMailList({
      query: "",
      filter: "unread",
      onFilterChange: null,
      onSync: null,
    });
    expect(screen.getByText("这个筛选条件下暂时没有邮件")).toBeTruthy();
    expect(screen.queryByText("切换上方筛选条件查看其他邮件")).toBeNull();
  });

  it("announces count, progress, empty, and failure changes with matching urgency", () => {
    const { container, rerenderMailList } = renderMailList({
      messages: [firstMessage],
      loadState: { phase: "syncing", completed: 1, total: 3 },
    });
    const count = screen.getByRole("status", {
      name: "收件箱当前显示 1 封邮件",
    });
    expect(count.getAttribute("aria-live")).toBe("polite");
    expect(count.getAttribute("aria-atomic")).toBe("true");
    const progress = container.querySelector(".mail-load-progress");
    expect(progress.getAttribute("role")).toBe("status");
    expect(progress.getAttribute("aria-live")).toBe("polite");

    rerenderMailList({
      messages: [],
      loadState: { phase: "error", completed: 0, total: null },
    });
    const failure = container.querySelector(
      '[data-empty-state="retryable_failure"]',
    );
    expect(failure.getAttribute("role")).toBe("alert");
    expect(failure.getAttribute("aria-live")).toBe("assertive");

    rerenderMailList({
      messages: [],
      loadState: { phase: "ready", completed: 0, total: null },
      query: "不存在",
    });
    const searchEmpty = container.querySelector(
      '[data-empty-state="search_empty"]',
    );
    expect(searchEmpty.getAttribute("role")).toBe("status");
    expect(searchEmpty.getAttribute("aria-live")).toBe("polite");
  });

  it("renders mailbox capability setup instead of a fake empty Archive folder", async () => {
    const user = userEvent.setup();
    const onMailboxSetup = vi.fn();
    renderMailList({
      folderRole: "archive",
      mailboxCapability: {
        role: "archive",
        status: "needs_creation_confirmation",
        retryable: true,
      },
      onMailboxSetup,
    });

    expect(screen.getByText("需要设置归档邮箱")).toBeTruthy();
    expect(screen.queryByText("归档里还没有邮件")).toBeNull();
    await user.click(screen.getByRole("button", { name: "设置归档邮箱" }));
    expect(onMailboxSetup).toHaveBeenCalledWith("archive");
  });

  it("shows a precise unavailable reason without inventing a retry action", () => {
    renderMailList({
      folderRole: "trash",
      mailboxCapability: {
        role: "trash",
        status: "unavailable",
        unavailable_reason: "provider_unsupported",
        retryable: false,
      },
    });

    expect(screen.getByText("垃圾箱当前不可用")).toBeTruthy();
    expect(screen.getByText("当前邮箱服务不支持此功能")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /重新确认/ })).toBeNull();
  });
});

describe("MailList pagination and semantics", () => {
  afterEach(cleanup);

  it("exposes controlled load-more states and only claims the end when confirmed", async () => {
    const user = userEvent.setup();
    const onLoadMore = vi.fn();
    const { rerenderMailList } = renderMailList({
      messages: [firstMessage],
      onLoadMore,
      loadMoreState: "idle",
    });

    await user.click(screen.getByRole("button", { name: "加载更早邮件" }));
    expect(onLoadMore).toHaveBeenCalledOnce();

    rerenderMailList({ loadMoreState: "loading" });
    expect(screen.getByText("正在加载更早邮件…")).toBeTruthy();

    rerenderMailList({ loadMoreState: "retry" });
    const retry = screen
      .getByRole("button", { name: "重试加载更早邮件" })
      .closest(".mail-load-progress");
    expect(retry.getAttribute("role")).toBe("alert");
    expect(retry.getAttribute("aria-live")).toBe("assertive");
    await user.click(
      screen.getByRole("button", { name: "重试加载更早邮件" }),
    );
    expect(onLoadMore).toHaveBeenCalledTimes(2);

    rerenderMailList({ loadMoreState: "offline" });
    expect(
      screen.getByText("连接网络后可继续加载更早邮件"),
    ).toBeTruthy();

    rerenderMailList({
      loadMoreState: "complete",
      endReached: false,
    });
    expect(screen.queryByText("已显示全部邮件")).toBeNull();

    rerenderMailList({
      loadMoreState: "complete",
      endReached: true,
    });
    expect(screen.getByText("已显示全部邮件")).toBeTruthy();
  });

  it("maps every backend remote history state without inventing an end state", () => {
    const onLoadMore = vi.fn();
    const { rerenderMailList } = renderMailList({
      messages: [firstMessage],
      onLoadMore,
      loadMoreState: { remote_history_state: "not_checked" },
    });

    expect(
      screen.getByRole("button", { name: "加载更早邮件" }),
    ).toBeTruthy();

    rerenderMailList({
      loadMoreState: { remote_history_state: "may_have_more" },
    });
    expect(
      screen.getByRole("button", { name: "加载更早邮件" }),
    ).toBeTruthy();

    rerenderMailList({
      loadMoreState: {
        phase: "idle",
        remote_history_state: "offline",
      },
    });
    expect(
      screen.getByText("连接网络后可继续加载更早邮件"),
    ).toBeTruthy();

    rerenderMailList({
      loadMoreState: { remote_history_state: "complete" },
      endReached: false,
    });
    expect(screen.queryByText("已显示全部邮件")).toBeNull();

    rerenderMailList({
      loadMoreState: { remote_history_state: "complete" },
      endReached: true,
    });
    expect(screen.getByText("已显示全部邮件")).toBeTruthy();

    rerenderMailList({
      loadMoreState: { remote_history_state: "unavailable" },
      endReached: false,
    });
    expect(
      screen.getByText("此文件夹无法提供更多历史邮件"),
    ).toBeTruthy();
    expect(screen.queryByText("已显示全部邮件")).toBeNull();
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
