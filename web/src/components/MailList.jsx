import {
  ArrowClockwise,
  CircleNotch,
  FunnelSimple,
  List,
  MagnifyingGlass,
  Star,
} from "@phosphor-icons/react";
import { useEffect, useRef, useState } from "react";
import { IconButton } from "./IconButton.jsx";
import { ProfileAvatar } from "./ProfileAvatar.jsx";
import { TooltipTarget } from "./Tooltip.jsx";
import {
  formatMailTime,
  hasFlag,
  senderLabel,
} from "../utils/formatters.js";
import { messageNavigationKey } from "../utils/messageNavigation.js";
import { useSlidingSelection } from "../hooks/useSlidingSelection.js";

const folderConfigurations = {
  inbox: {
    title: "收件箱",
    eyebrow: "INBOX",
    syncLabel: "同步收件箱",
    syncingLabel: "正在同步收件箱",
    tabs: [
      { id: "all", label: "全部" },
      { id: "unread", label: "未读" },
      { id: "starred", label: "收藏" },
    ],
    supportsSeen: true,
    supportsStar: true,
  },
  starred: {
    title: "已收藏",
    eyebrow: "STARRED",
    syncLabel: "同步已收藏邮件",
    syncingLabel: "正在同步已收藏邮件",
    tabs: [
      { id: "all", label: "全部" },
      { id: "unread", label: "未读" },
    ],
    supportsSeen: true,
    supportsStar: true,
  },
  sent: {
    title: "已发送",
    eyebrow: "SENT",
    syncLabel: "同步已发送",
    syncingLabel: "正在同步已发送",
    tabs: [
      { id: "all", label: "全部" },
      { id: "starred", label: "收藏" },
    ],
    supportsSeen: true,
    supportsStar: true,
  },
  drafts: {
    title: "草稿",
    eyebrow: "DRAFTS",
    syncLabel: "同步草稿",
    syncingLabel: "正在同步草稿",
    tabs: [],
    supportsSeen: false,
    supportsStar: false,
  },
  outbox: {
    title: "发件队列",
    eyebrow: "OUTBOX",
    syncLabel: "刷新发件队列",
    syncingLabel: "正在刷新发件队列",
    tabs: [],
    supportsSeen: false,
    supportsStar: false,
  },
  archive: {
    title: "归档",
    eyebrow: "ARCHIVE",
    syncLabel: "同步归档",
    syncingLabel: "正在同步归档",
    mailboxLabel: "归档文件夹",
    tabs: [
      { id: "all", label: "全部" },
      { id: "unread", label: "未读" },
      { id: "starred", label: "收藏" },
    ],
    supportsSeen: true,
    supportsStar: true,
  },
  trash: {
    title: "垃圾箱",
    eyebrow: "TRASH",
    syncLabel: "同步垃圾箱",
    syncingLabel: "正在同步垃圾箱",
    mailboxLabel: "垃圾箱",
    tabs: [
      { id: "all", label: "全部" },
      { id: "unread", label: "未读" },
    ],
    supportsSeen: true,
    supportsStar: false,
  },
};

const folderRoleByTitle = Object.fromEntries(
  Object.entries(folderConfigurations).map(([role, config]) => [
    config.title,
    role,
  ]),
);

function resolvedFolderRole(folderRole, folderLabel) {
  if (folderConfigurations[folderRole]) return folderRole;
  return folderRoleByTitle[folderLabel] || "inbox";
}

function resolvedPaginationPhase(loadMoreState) {
  const operationPhase =
    typeof loadMoreState === "object"
      ? loadMoreState?.phase || loadMoreState?.state
      : null;
  const remoteHistoryState =
    typeof loadMoreState === "object"
      ? loadMoreState?.remote_history_state ||
        loadMoreState?.remoteHistoryState
      : null;
  const phase =
    typeof loadMoreState === "string"
      ? loadMoreState
      : ["loading", "retry"].includes(operationPhase)
        ? operationPhase
        : remoteHistoryState || operationPhase;
  const normalizedPhase =
    {
      may_have_more: "idle",
      not_checked: "idle",
    }[phase] || phase;
  return [
    "idle",
    "loading",
    "retry",
    "offline",
    "complete",
    "unavailable",
  ].includes(normalizedPhase)
    ? normalizedPhase
    : "idle";
}

function PaginationControl({ phase, visibleCount }) {
  const previousPhaseRef = useRef(phase);
  const loadStartCountRef = useRef(visibleCount);
  const dismissTimerRef = useRef(null);
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    const previousPhase = previousPhaseRef.current;
    previousPhaseRef.current = phase;

    if (dismissTimerRef.current) {
      window.clearTimeout(dismissTimerRef.current);
      dismissTimerRef.current = null;
    }

    if (phase === "loading") {
      loadStartCountRef.current = visibleCount;
      setNotice({ state: "loading", text: "正在加载…" });
      return undefined;
    }

    if (previousPhase !== "loading") return undefined;

    const failed = ["retry", "offline", "unavailable"].includes(phase);
    const appendedCount = Math.max(0, visibleCount - loadStartCountRef.current);
    setNotice({
      state: failed ? "error" : "complete",
      text: failed
        ? "加载失败"
        : appendedCount
          ? `已加载 ${appendedCount} 封`
          : "加载完成",
    });
    dismissTimerRef.current = window.setTimeout(() => {
      setNotice(null);
      dismissTimerRef.current = null;
    }, 2_000);
    return undefined;
  }, [phase, visibleCount]);

  useEffect(
    () => () => {
      if (dismissTimerRef.current) {
        window.clearTimeout(dismissTimerRef.current);
      }
    },
    [],
  );

  if (!notice) return null;
  return (
    <div
      className="mail-pagination-notice"
      data-state={notice.state}
      role={notice.state === "error" ? "alert" : "status"}
      aria-live={notice.state === "error" ? "assertive" : "polite"}
      aria-atomic="true"
    >
      {notice.state === "loading" ? (
        <CircleNotch size={13} aria-hidden="true" />
      ) : null}
      <span>{notice.text}</span>
    </div>
  );
}

export function MailList({
  folderRole,
  folderLabel,
  messages = [],
  selectedMessageId,
  selectedMessage = null,
  onSelect = null,
  onToggleStar = null,
  query = "",
  onQueryChange = null,
  filter = "all",
  onFilterChange = null,
  onOpenFilters = null,
  filterDisabledReason = null,
  onSync = null,
  syncState = "idle",
  loadState = { phase: "ready", completed: 0, total: null },
  canSync = true,
  syncDisabledReason = null,
  onOpenMobileNav = null,
  avatarForEmail = () => null,
  displayNameForEmail = () => null,
  referenceJump = null,
  mailboxCapability = null,
  loadMoreState = "idle",
  onLoadMore = null,
}) {
  const messageListRef = useRef(null);
  const loadMoreSentinelRef = useRef(null);
  const autoLoadRequestedRef = useRef(false);
  const mailRowsRef = useRef(null);
  const role = resolvedFolderRole(folderRole, folderLabel);
  const config = folderConfigurations[role];
  const title = folderLabel || config.title;
  const isSyncing = syncState === "syncing" || loadState.phase === "syncing";
  const capabilityUnavailable = Boolean(
    config.mailboxLabel &&
      mailboxCapability &&
      mailboxCapability.status !== "available",
  );
  const paginationPhase = resolvedPaginationPhase(loadMoreState);
  const selectedNavigationKey = messageNavigationKey(selectedMessage);
  const selectedRowKey =
    selectedNavigationKey ||
    (typeof selectedMessageId === "string" ? selectedMessageId : null);
  const mailRowsLayoutKey = messages
    .map((message, index) => messageNavigationKey(message) || `legacy-${index}`)
    .join("|");
  const {
    motionReady: rowSelectionMotionReady,
    selectionStyle: rowSelectionStyle,
    selectionVisible: rowSelectionVisible,
  } = useSlidingSelection({
    containerRef: mailRowsRef,
    layoutKey: mailRowsLayoutKey,
    selectedKey: selectedRowKey,
  });
  const canSearch =
    typeof onQueryChange === "function" && !capabilityUnavailable;
  const visibleTabs =
    typeof onFilterChange === "function" && !capabilityUnavailable
      ? config.tabs
      : [];
  const showFilterMenu =
    !capabilityUnavailable &&
    (typeof onOpenFilters === "function" || Boolean(filterDisabledReason));
  const syncIsDisabled =
    isSyncing || !canSync || capabilityUnavailable;
  const requestOlderPage = () => {
    if (
      autoLoadRequestedRef.current ||
      typeof onLoadMore !== "function" ||
      paginationPhase !== "idle"
    ) {
      return;
    }
    autoLoadRequestedRef.current = true;
    onLoadMore();
  };

  useEffect(() => {
    if (!referenceJump?.key || !messageListRef.current) return;
    const targetRow = Array.from(
      messageListRef.current.querySelectorAll(".mail-row"),
    ).find((row) => row.dataset.navigationKey === referenceJump.key);
    if (!targetRow) return;
    targetRow.scrollIntoView?.({ block: "nearest" });
    // The list item is programmatically focusable for reference jumps; normal
    // keyboard navigation lands on its native open/star buttons.
    targetRow.focus({ preventScroll: true });
  }, [referenceJump]);

  useEffect(() => {
    if (paginationPhase !== "idle") autoLoadRequestedRef.current = false;
  }, [paginationPhase]);

  useEffect(() => {
    const root = messageListRef.current;
    const sentinel = loadMoreSentinelRef.current;
    if (
      !root ||
      !sentinel ||
      typeof onLoadMore !== "function" ||
      paginationPhase !== "idle" ||
      typeof IntersectionObserver !== "function"
    ) {
      return undefined;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) requestOlderPage();
      },
      {
        root,
        rootMargin: "0px 0px 64px",
        threshold: 0,
      },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [messages.length, onLoadMore, paginationPhase]);

  return (
    <section className="mail-list-panel" aria-label={`${title}邮件列表`}>
      <div className="list-topbar">
        {typeof onOpenMobileNav === "function" ? (
          <button
            type="button"
            className="mobile-nav-button"
            aria-label="打开导航"
            onClick={onOpenMobileNav}
          >
            <List size={21} aria-hidden="true" />
          </button>
        ) : null}
        {canSearch ? (
          <label className="search-box inset-input-shell">
            <MagnifyingGlass size={18} aria-hidden="true" />
            <input
              value={query}
              autoComplete="off"
              onChange={(event) => onQueryChange(event.target.value)}
              placeholder="搜索已同步邮件"
              aria-label="搜索邮件"
              aria-description="范围：搜索已同步邮件"
            />
          </label>
        ) : null}
        {showFilterMenu ? (
          <IconButton
            label={
              filterDisabledReason
                ? `筛选邮件不可用：${filterDisabledReason}`
                : "筛选邮件"
            }
            title={filterDisabledReason || "筛选邮件"}
            onClick={onOpenFilters || undefined}
            disabled={typeof onOpenFilters !== "function"}
          >
            <FunnelSimple size={19} />
          </IconButton>
        ) : null}
      </div>

      <div className="list-heading">
        <div>
          <p className="eyebrow">{config.eyebrow}</p>
          <h1>{title}</h1>
        </div>
        {typeof onSync === "function" ? (
          <IconButton
            label={isSyncing ? config.syncingLabel : config.syncLabel}
            title={
              syncIsDisabled && !isSyncing
                ? syncDisabledReason || undefined
                : undefined
            }
            onClick={onSync}
            disabled={syncIsDisabled}
            aria-busy={isSyncing || undefined}
            className={isSyncing ? "is-spinning" : ""}
          >
            <ArrowClockwise size={19} />
          </IconButton>
        ) : null}
      </div>

      <div className="mail-tabs" aria-label="邮件列表状态">
        {visibleTabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            className="mail-tab"
            data-selected={filter === tab.id}
            aria-pressed={filter === tab.id}
            onClick={() => onFilterChange(tab.id)}
          >
            {tab.label}
          </button>
        ))}
        <span
          className="mail-tabs__count"
          role="status"
          aria-live="polite"
          aria-atomic="true"
          aria-label={`${title}当前显示 ${messages.length} 封邮件`}
        >
          {messages.length} 封
        </span>
      </div>

      <div
        className="message-list vertical-scroll-surface"
        ref={messageListRef}
        onScroll={(event) => {
          const surface = event.currentTarget;
          const remaining =
            surface.scrollHeight - surface.scrollTop - surface.clientHeight;
          if (remaining <= 64) requestOlderPage();
        }}
      >
        {messages.length ? (
          <>
            <ul
              ref={mailRowsRef}
              className="mail-list"
              aria-label="邮件"
              data-selection-visible={rowSelectionVisible || undefined}
              data-selection-motion-ready={
                rowSelectionMotionReady || undefined
              }
              style={rowSelectionStyle}
            >
              {messages.map((message, index) => {
                const navigationKey = messageNavigationKey(message);
                const rowMessageId =
                  typeof message.id === "string"
                    ? message.id.trim() || null
                    : null;
                const selected =
                  navigationKey && selectedNavigationKey
                    ? navigationKey === selectedNavigationKey
                    : rowMessageId !== null &&
                      rowMessageId === selectedMessageId;
                const unread =
                  config.supportsSeen && !hasFlag(message, "\\Seen");
                const starred = hasFlag(message, "\\Flagged");
                const listSender = message.list_sender || message.sender;
                const sender =
                  displayNameForEmail(listSender?.email)?.trim() ||
                  listSender?.name ||
                  listSender?.email ||
                  senderLabel(message);
                const canToggleStar =
                  rowMessageId !== null &&
                  config.supportsStar &&
                  typeof onToggleStar === "function" &&
                  message.kind !== "draft" &&
                  message.kind !== "outbox";
                const subject = message.subject || "（无主题）";
                const canOpen =
                  rowMessageId !== null && typeof onSelect === "function";

                return (
                  <li
                    key={navigationKey || `legacy-row-${index}`}
                    className="mail-row"
                    data-selected={selected}
                    data-unread={unread}
                    data-navigation-key={navigationKey || undefined}
                    tabIndex={-1}
                    style={{ "--row-index": index }}
                    onClick={(event) => {
                      if (
                        canOpen &&
                        !event.target.closest?.("button")
                      ) {
                        onSelect(message);
                      }
                    }}
                  >
                    {canOpen ? (
                      <button
                        type="button"
                        className="mail-row__open"
                        aria-label={`打开邮件：${sender}，${subject}`}
                        aria-current={selected ? "true" : undefined}
                        onClick={() => onSelect(message)}
                      />
                    ) : null}
                    <span className="mail-row__visual" aria-hidden="true">
                      <ProfileAvatar
                        className="mail-row__avatar"
                        email={listSender?.email}
                        label={sender}
                        customSrc={avatarForEmail(listSender?.email)}
                      />
                      <span className="mail-row__content">
                        <span className="mail-row__meta">
                          <span className="mail-row__sender">
                            {unread ? <span className="unread-dot" /> : null}
                            {sender}
                          </span>
                          <time dateTime={message.sent_at}>
                            {formatMailTime(message.sent_at)}
                          </time>
                        </span>
                        <span className="mail-row__subject">{subject}</span>
                        <span className="mail-row__preview">
                          {message.preview || "暂无摘要"}
                        </span>
                      </span>
                    </span>
                    {canToggleStar ? (
                      <TooltipTarget
                        label={starred ? "取消收藏" : "添加收藏"}
                      >
                        <button
                          type="button"
                          className="star-button"
                          data-active={starred}
                          aria-label={
                            starred
                              ? `取消收藏：${subject}`
                              : `添加收藏：${subject}`
                          }
                          aria-pressed={starred}
                          onClick={() => onToggleStar(message)}
                        >
                          <Star
                            size={17}
                            weight={starred ? "fill" : "regular"}
                          />
                        </button>
                      </TooltipTarget>
                    ) : null}
                  </li>
                );
              })}
            </ul>
            <span
              ref={loadMoreSentinelRef}
              className="mail-pagination-sentinel"
              aria-hidden="true"
            />
            <PaginationControl
              phase={paginationPhase}
              visibleCount={messages.length}
            />
          </>
        ) : null}
      </div>
    </section>
  );
}
