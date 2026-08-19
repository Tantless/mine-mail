import {
  ArrowClockwise,
  CheckCircle,
  CircleNotch,
  FunnelSimple,
  List,
  MagnifyingGlass,
  SidebarSimple,
  Star,
  WarningCircle,
} from "@phosphor-icons/react";
import { useEffect, useLayoutEffect, useRef } from "react";
import { IconButton } from "./IconButton.jsx";
import { ProfileAvatar } from "./ProfileAvatar.jsx";
import { TooltipTarget } from "./Tooltip.jsx";
import {
  formatMailTime,
  hasFlag,
  senderLabel,
} from "../utils/formatters.js";
import { messageNavigationKey } from "../utils/messageNavigation.js";
import { userFacingErrorMessage } from "../utils/userFacingError.js";
import { limitText, textInputLimits } from "../utils/textLimits.js";
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
    tabs: [],
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

function SyncFeedbackRow({ feedback }) {
  if (!feedback) return null;
  const state = ["syncing", "success", "error"].includes(feedback.state)
    ? feedback.state
    : "syncing";
  const message =
    state === "error"
      ? userFacingErrorMessage(feedback.message, "邮箱同步没有完成")
      : feedback.message;

  return (
    <div
      className="mail-sync-feedback"
      data-state={state}
      role={state === "error" ? "alert" : "status"}
      aria-live={state === "error" ? "assertive" : "polite"}
      aria-atomic="true"
    >
      {state === "syncing" ? (
        <CircleNotch size={14} weight="regular" aria-hidden="true" />
      ) : null}
      <span>{message}</span>
    </div>
  );
}

function MailListCenterState({ state }) {
  const configuration = {
    loading: {
      label: "加载中…",
      icon: CircleNotch,
      role: "status",
    },
    success: {
      label: "加载成功",
      icon: CheckCircle,
      role: "status",
    },
    error: {
      label: "加载失败，请点击右上角重试",
      icon: WarningCircle,
      role: "alert",
    },
    search: {
      label: "没有匹配的已同步邮件",
      icon: null,
      role: "status",
    },
    filter: {
      label: "没有符合当前筛选条件的邮件",
      icon: null,
      role: "status",
    },
  }[state];
  if (!configuration) return null;
  const StateIcon = configuration.icon;

  return (
    <div
      className="mail-list-center-state"
      data-state={state}
      role={configuration.role}
      aria-live={state === "error" ? "assertive" : "polite"}
      aria-atomic="true"
      aria-busy={state === "loading" || undefined}
    >
      {StateIcon ? (
        <StateIcon size={18} weight="regular" aria-hidden="true" />
      ) : null}
      <span>{configuration.label}</span>
    </div>
  );
}

export function MailList({
  folderRole,
  folderLabel,
  messages = [],
  hasFolderMessages,
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
  onCollapse = null,
  onSync = null,
  syncState = "idle",
  syncFeedback = null,
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
  scrollStateKey = null,
  getScrollTop = null,
  onScrollTopChange = null,
}) {
  const messageListRef = useRef(null);
  const loadMoreSentinelRef = useRef(null);
  const autoLoadRequestedRef = useRef(false);
  const mailRowsRef = useRef(null);
  const role = resolvedFolderRole(folderRole, folderLabel);
  const config = folderConfigurations[role];
  const title = folderLabel || config.title;
  const isSyncing = syncState === "syncing";
  const folderHasMessages =
    typeof hasFolderMessages === "boolean"
      ? hasFolderMessages
      : messages.length > 0;
  const capabilityUnavailable = Boolean(
    config.mailboxLabel &&
      mailboxCapability &&
      mailboxCapability.status !== "available",
  );
  const paginationPhase = resolvedPaginationPhase(loadMoreState);
  const isLoadingMore = paginationPhase === "loading";
  const emptyFolderIsLoading =
    !folderHasMessages &&
    (["loading", "syncing"].includes(loadState?.phase) ||
      syncFeedback?.state === "syncing" ||
      paginationPhase === "loading");
  const emptyFolderHasFailed =
    !folderHasMessages &&
    (loadState?.phase === "error" ||
      syncFeedback?.state === "error" ||
      paginationPhase === "retry");
  const emptyFolderIsSettled =
    !folderHasMessages &&
    loadState?.phase === "ready" &&
    paginationPhase === "complete" &&
    !capabilityUnavailable;
  const centerState =
    messages.length > 0
      ? null
      : emptyFolderIsLoading
        ? "loading"
        : emptyFolderHasFailed
          ? "error"
          : query.trim()
            ? "search"
            : filter !== "all"
              ? "filter"
              : emptyFolderIsSettled
                ? "success"
                : null;
  const visibleSyncFeedback = folderHasMessages ? syncFeedback : null;
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

  useLayoutEffect(() => {
    const surface = messageListRef.current;
    if (!surface || scrollStateKey === null) return undefined;
    const storedScrollTop =
      typeof getScrollTop === "function"
        ? getScrollTop(scrollStateKey)
        : 0;
    surface.scrollTop =
      Number.isFinite(storedScrollTop) && storedScrollTop > 0
        ? storedScrollTop
        : 0;

    return () => {
      if (typeof onScrollTopChange === "function") {
        onScrollTopChange(scrollStateKey, surface.scrollTop);
      }
    };
  }, [getScrollTop, onScrollTopChange, scrollStateKey]);

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
    autoLoadRequestedRef.current = false;
  }, [scrollStateKey]);

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
        const isIntersecting = entries.some((entry) => entry.isIntersecting);
        if (!isIntersecting) {
          autoLoadRequestedRef.current = false;
          return;
        }
        requestOlderPage();
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

  const hasVisibleTabs = visibleTabs.length > 0;
  const messageCount = (
    <span
      className="mail-tabs__count"
      role="status"
      aria-live="polite"
      aria-atomic="true"
      aria-label={`${title}当前显示 ${messages.length} 封邮件`}
    >
      {messages.length} 封
    </span>
  );

  return (
    <section
      id="mail-list-panel"
      className="mail-list-panel"
      aria-label={`${title}邮件列表`}
    >
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
              maxLength={textInputLimits.mailSearch}
              autoComplete="off"
              onChange={(event) =>
                onQueryChange(
                  limitText(event.target.value, textInputLimits.mailSearch),
                )
              }
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
        {typeof onCollapse === "function" ? (
          <IconButton
            label="收起邮件列表"
            onClick={onCollapse}
            aria-controls="mail-list-panel"
            aria-expanded="true"
          >
            <SidebarSimple size={19} aria-hidden="true" />
          </IconButton>
        ) : null}
      </div>

      <div className="list-heading">
        <div>
          <p className="eyebrow">{config.eyebrow}</p>
          <h1>{title}</h1>
        </div>
        {typeof onSync === "function" || !hasVisibleTabs ? (
          <div className="list-heading__meta">
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
            {!hasVisibleTabs ? messageCount : null}
          </div>
        ) : null}
      </div>

      <div className="mail-list-status-region">
        <div
          className="mail-tabs"
          data-compact={!hasVisibleTabs || undefined}
          aria-label={hasVisibleTabs ? "邮件列表状态" : undefined}
          aria-hidden={!hasVisibleTabs || undefined}
        >
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
          {hasVisibleTabs ? messageCount : null}
        </div>
        <SyncFeedbackRow feedback={visibleSyncFeedback} />
      </div>

      <div
        className="message-list vertical-scroll-surface"
        data-centered-state={centerState || undefined}
        aria-busy={centerState === "loading" || undefined}
        ref={messageListRef}
        onScroll={(event) => {
          const surface = event.currentTarget;
          if (
            scrollStateKey !== null &&
            typeof onScrollTopChange === "function"
          ) {
            onScrollTopChange(scrollStateKey, surface.scrollTop);
          }
          const remaining =
            surface.scrollHeight - surface.scrollTop - surface.clientHeight;
          if (remaining > 64) {
            autoLoadRequestedRef.current = false;
          } else {
            requestOlderPage();
          }
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
                            <span className="mail-row__sender-text">
                              {sender}
                            </span>
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
            <div
              ref={loadMoreSentinelRef}
              className="mail-pagination-sentinel"
              data-state={paginationPhase}
              role={isLoadingMore ? "status" : undefined}
              aria-label={isLoadingMore ? "正在加载更多邮件" : undefined}
              aria-live={isLoadingMore ? "polite" : undefined}
              aria-atomic={isLoadingMore || undefined}
              aria-busy={isLoadingMore || undefined}
              aria-hidden={isLoadingMore ? undefined : "true"}
            >
              {isLoadingMore ? (
                <>
                  <CircleNotch size={14} weight="regular" aria-hidden="true" />
                  <span>正在加载更多邮件…</span>
                </>
              ) : null}
            </div>
          </>
        ) : (
          <MailListCenterState state={centerState} />
        )}
      </div>
    </section>
  );
}
