import {
  ArrowClockwise,
  CircleNotch,
  FunnelSimple,
  List,
  MagnifyingGlass,
  Star,
} from "@phosphor-icons/react";
import { useEffect, useRef } from "react";
import { IconButton } from "./IconButton.jsx";
import { ProfileAvatar } from "./ProfileAvatar.jsx";
import { TooltipTarget } from "./Tooltip.jsx";
import {
  formatMailTime,
  hasFlag,
  senderLabel,
} from "../utils/formatters.js";
import { messageNavigationKey } from "../utils/messageNavigation.js";

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
    emptyTitle: "收件箱里还没有邮件",
    emptyDetail: "新邮件会在同步后显示在这里",
    notSyncedTitle: "收件箱尚未同步",
    notSyncedDetail: "完成首次同步后，收件箱邮件会显示在这里",
    notSyncedAction: "使用“同步收件箱”获取邮件",
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
    emptyTitle: "还没有收藏邮件",
    emptyDetail: "收藏的邮件会集中显示在这里",
    notSyncedTitle: "收藏来源尚未同步",
    notSyncedDetail: "完成首次同步后，已收藏邮件会集中显示在这里",
    notSyncedAction: "使用“同步已收藏邮件”获取邮件",
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
    emptyTitle: "还没有已发送邮件",
    emptyDetail: "成功发送的邮件会显示在这里",
    notSyncedTitle: "已发送尚未同步",
    notSyncedDetail: "完成首次同步后，已发送邮件会显示在这里",
    notSyncedAction: "使用“同步已发送”获取邮件",
    supportsSeen: true,
    supportsStar: true,
  },
  drafts: {
    title: "草稿",
    eyebrow: "DRAFTS",
    syncLabel: "同步草稿",
    syncingLabel: "正在同步草稿",
    tabs: [],
    emptyTitle: "还没有草稿",
    emptyDetail: "保存的草稿会显示在这里",
    notSyncedTitle: "草稿尚未同步",
    notSyncedDetail: "完成首次同步后，已保存草稿会显示在这里",
    notSyncedAction: "使用“同步草稿”读取已保存草稿",
    supportsSeen: false,
    supportsStar: false,
  },
  outbox: {
    title: "发件队列",
    eyebrow: "OUTBOX",
    syncLabel: "刷新发件队列",
    syncingLabel: "正在刷新发件队列",
    tabs: [],
    emptyTitle: "发件队列为空",
    emptyDetail: "待发送或需要处理的邮件会显示在这里",
    notSyncedTitle: "发件队列尚未读取",
    notSyncedDetail: "完成首次读取后，待处理邮件会显示在这里",
    notSyncedAction: "使用“刷新发件队列”读取待处理邮件",
    supportsSeen: false,
    supportsStar: false,
  },
  archive: {
    title: "归档",
    eyebrow: "ARCHIVE",
    syncLabel: "同步归档",
    syncingLabel: "正在同步归档",
    mailboxLabel: "归档邮箱",
    tabs: [
      { id: "all", label: "全部" },
      { id: "unread", label: "未读" },
      { id: "starred", label: "收藏" },
    ],
    emptyTitle: "归档里还没有邮件",
    emptyDetail: "归档后的邮件会显示在这里",
    notSyncedTitle: "归档尚未同步",
    notSyncedDetail: "完成首次同步后，归档邮件会显示在这里",
    notSyncedAction: "使用“同步归档”获取邮件",
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
    emptyTitle: "垃圾箱里还没有邮件",
    emptyDetail: "移到垃圾箱的邮件会显示在这里",
    notSyncedTitle: "垃圾箱尚未同步",
    notSyncedDetail: "完成首次同步后，垃圾箱邮件会显示在这里",
    notSyncedAction: "使用“同步垃圾箱”获取邮件",
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

const capabilityFailureCopy = {
  create_not_supported: "服务器不支持创建所需邮箱",
  create_failed: "邮箱创建失败",
  created_mailbox_not_selectable: "已创建的邮箱无法打开",
  provider_unsupported: "当前邮箱服务不支持此功能",
};

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

function capabilityEmptyState(role, config, capability) {
  if (!config.mailboxLabel || !capability || capability.status === "available") {
    return null;
  }

  if (capability.status === "discovery_pending") {
    return {
      title: `正在确认${config.mailboxLabel}…`,
      detail: "确认完成前不会把未知邮箱当作这个文件夹",
      action: capability.retryable ? "retry" : null,
      actionLabel: `重新确认${config.mailboxLabel}`,
      liveRole: "status",
      role,
    };
  }

  if (capability.status === "needs_creation_confirmation") {
    return {
      title: `需要设置${config.mailboxLabel}`,
      detail: `创建并确认${config.mailboxLabel}后才能使用此功能`,
      action: "setup",
      actionLabel: `设置${config.mailboxLabel}`,
      liveRole: "status",
      role,
    };
  }

  return {
    title: `${config.title}当前不可用`,
    detail:
      capabilityFailureCopy[capability.unavailable_reason] ||
      "此账户暂时不能使用该文件夹",
    action: capability.retryable ? "retry" : null,
    actionLabel: `重新确认${config.mailboxLabel}`,
    liveRole: "alert",
    role,
  };
}

function PaginationControl({
  endReached,
  failureReason,
  onLoadMore,
  phase,
}) {
  if (endReached) {
    return (
      <div
        className="mail-load-progress"
        data-pagination-state="complete"
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        <span>已显示全部邮件</span>
      </div>
    );
  }

  if (phase === "loading") {
    return (
      <div
        className="mail-load-progress"
        data-pagination-state="loading"
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        <CircleNotch size={15} aria-hidden="true" />
        <span>正在加载更早邮件…</span>
      </div>
    );
  }

  if (phase === "offline") {
    return (
      <div
        className="mail-load-progress"
        data-pagination-state="offline"
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        <span>连接网络后可继续加载更早邮件</span>
      </div>
    );
  }

  if (phase === "retry") {
    return (
      <div
        className="mail-load-progress"
        data-pagination-state="retry"
        role="alert"
        aria-live="assertive"
        aria-atomic="true"
      >
        {typeof onLoadMore === "function" ? (
          <button
            type="button"
            className="secondary-button"
            onClick={onLoadMore}
          >
            <ArrowClockwise size={16} aria-hidden="true" />
            重试加载更早邮件
          </button>
        ) : (
          <span>{failureReason || "更早邮件暂时无法加载"}</span>
        )}
      </div>
    );
  }

  if (phase === "unavailable") {
    return (
      <div
        className="mail-load-progress"
        data-pagination-state="unavailable"
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        <span>{failureReason || "此文件夹无法提供更多历史邮件"}</span>
      </div>
    );
  }

  if (phase === "idle" && typeof onLoadMore === "function") {
    return (
      <div
        className="mail-load-progress"
        data-pagination-state="idle"
      >
        <button
          type="button"
          className="secondary-button"
          onClick={onLoadMore}
        >
          加载更早邮件
        </button>
      </div>
    );
  }

  // A "complete" hint is not authoritative without end_reached=true.
  return null;
}

function EmptyListState({
  capabilityState,
  canChangeFilter,
  canLoadMore,
  canSync,
  config,
  emptyState,
  isLoading,
  onMailboxCapabilityRetry,
  onMailboxSetup,
  onSync,
  progressLabel,
}) {
  if (capabilityState) {
    const callback =
      capabilityState.action === "setup"
        ? onMailboxSetup
        : capabilityState.action === "retry"
          ? onMailboxCapabilityRetry
          : null;
    return (
      <div
        className="empty-list"
        data-empty-state="capability"
        role={capabilityState.liveRole || "status"}
        aria-live={
          capabilityState.liveRole === "alert" ? "assertive" : "polite"
        }
        aria-atomic="true"
      >
        <ArrowClockwise size={26} aria-hidden="true" />
        <strong>{capabilityState.title}</strong>
        <span>{capabilityState.detail}</span>
        {typeof callback === "function" ? (
          <button
            type="button"
            className="secondary-button"
            onClick={() => callback(capabilityState.role)}
          >
            {capabilityState.actionLabel}
          </button>
        ) : null}
      </div>
    );
  }

  if (emptyState === "initial_sync" || isLoading) {
    return (
      <div
        className="mail-loading-state"
        data-empty-state="initial_sync"
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        <CircleNotch size={24} aria-hidden="true" />
        <strong>{progressLabel}</strong>
        <span>会先显示本地缓存，再补充服务器邮件</span>
        <div className="mail-loading-skeletons" aria-hidden="true">
          {[0, 1, 2].map((index) => (
            <span className="mail-loading-skeleton" key={index}>
              <i />
              <b />
            </span>
          ))}
        </div>
      </div>
    );
  }

  const stateCopy = {
    search_empty: {
      title: "没有匹配的已同步邮件",
      detail: canLoadMore
        ? "换个关键词，或加载更多历史邮件后再搜索"
        : canSync && typeof onSync === "function"
          ? "换个关键词，或同步更多邮件后再搜索"
          : "换个关键词后重试",
      icon: MagnifyingGlass,
    },
    filter_empty: {
      title: "当前筛选下没有邮件",
      detail: canChangeFilter
        ? "切换上方筛选条件查看其他邮件"
        : "这个筛选条件下暂时没有邮件",
      icon: MagnifyingGlass,
    },
    not_synced: {
      title: config.notSyncedTitle,
      detail:
        typeof onSync === "function"
          ? canSync
            ? config.notSyncedAction
            : `连接网络后，${config.notSyncedAction}`
          : config.notSyncedDetail,
      icon: ArrowClockwise,
    },
    offline_history_exhausted: {
      title: "已显示全部本地邮件",
      detail: "连接网络后可继续加载更早邮件",
      icon: ArrowClockwise,
    },
    loading_older: {
      title: "正在加载更早邮件…",
      detail: "当前已显示的邮件仍可使用",
      icon: CircleNotch,
    },
    retryable_failure: {
      title: "部分邮件暂时没有加载完成",
      detail:
        typeof onSync === "function"
          ? "可以使用同步按钮重试"
          : "稍后重新打开此文件夹重试",
      icon: ArrowClockwise,
    },
    confirmed_end: {
      title: config.emptyTitle,
      detail: "已确认没有更多邮件",
      icon: MagnifyingGlass,
    },
    true_empty: {
      title: config.emptyTitle,
      detail: config.emptyDetail,
      icon: MagnifyingGlass,
    },
  };
  const copy = stateCopy[emptyState] || stateCopy.true_empty;
  const StateIcon = copy.icon;
  const isFailure = emptyState === "retryable_failure";

  return (
    <div
      className="empty-list"
      data-empty-state={emptyState || "true_empty"}
      role={isFailure ? "alert" : "status"}
      aria-live={isFailure ? "assertive" : "polite"}
      aria-atomic="true"
    >
      <StateIcon size={26} aria-hidden="true" />
      <strong>{copy.title}</strong>
      <span>{copy.detail}</span>
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
  onMailboxSetup = null,
  onMailboxCapabilityRetry = null,
  emptyState = null,
  isInitialized = true,
  loadMoreState = "idle",
  onLoadMore = null,
  loadMoreFailureReason = null,
  endReached = false,
}) {
  const messageListRef = useRef(null);
  const role = resolvedFolderRole(folderRole, folderLabel);
  const config = folderConfigurations[role];
  const title = folderLabel || config.title;
  const isLoading =
    loadState.phase === "loading" || loadState.phase === "syncing";
  const isSyncing = syncState === "syncing" || loadState.phase === "syncing";
  const progressLabel =
    loadState.phase === "loading"
      ? `正在读取${title}本地邮件…`
      : loadState.total
        ? `${config.syncingLabel}，已加载 ${loadState.completed}/${loadState.total} 封`
        : `${config.syncingLabel}…`;
  const capabilityState = capabilityEmptyState(
    role,
    config,
    mailboxCapability,
  );
  const paginationPhase = resolvedPaginationPhase(loadMoreState);
  const selectedNavigationKey = messageNavigationKey(selectedMessage);
  const canSearch =
    typeof onQueryChange === "function" && capabilityState == null;
  const visibleTabs =
    typeof onFilterChange === "function" && capabilityState == null
      ? config.tabs
      : [];
  const showFilterMenu =
    capabilityState == null &&
    (typeof onOpenFilters === "function" || Boolean(filterDisabledReason));
  const syncIsDisabled =
    isSyncing || !canSync || capabilityState != null;

  let resolvedEmptyState = emptyState;
  if (!resolvedEmptyState && isLoading) {
    resolvedEmptyState = "initial_sync";
  } else if (!resolvedEmptyState && loadState.phase === "error") {
    resolvedEmptyState = "retryable_failure";
  } else if (!resolvedEmptyState && !isInitialized) {
    resolvedEmptyState = "not_synced";
  } else if (!resolvedEmptyState && query.trim()) {
    resolvedEmptyState = "search_empty";
  } else if (!resolvedEmptyState && filter !== "all") {
    resolvedEmptyState = "filter_empty";
  } else if (!resolvedEmptyState && paginationPhase === "loading") {
    resolvedEmptyState = "loading_older";
  } else if (!resolvedEmptyState && paginationPhase === "offline") {
    resolvedEmptyState = "offline_history_exhausted";
  } else if (!resolvedEmptyState && endReached) {
    resolvedEmptyState = "confirmed_end";
  } else if (!resolvedEmptyState) {
    resolvedEmptyState = "true_empty";
  }

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
                ? syncDisabledReason ||
                  capabilityState?.detail ||
                  "当前离线，无法同步"
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
      >
        {isLoading && messages.length ? (
          <div
            className="mail-load-progress"
            role="status"
            aria-live="polite"
            aria-atomic="true"
          >
            <CircleNotch size={15} aria-hidden="true" />
            <span>{progressLabel}</span>
          </div>
        ) : loadState.phase === "error" && messages.length ? (
          <div
            className="mail-load-progress"
            role="alert"
            aria-live="assertive"
            aria-atomic="true"
          >
            <span>部分邮件暂时没有加载完成</span>
          </div>
        ) : null}

        {messages.length ? (
          <>
            <ul
              className="mail-list"
              aria-label="邮件"
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
            <PaginationControl
              endReached={endReached}
              failureReason={loadMoreFailureReason}
              onLoadMore={onLoadMore}
              phase={paginationPhase}
            />
          </>
        ) : (
          <EmptyListState
            capabilityState={capabilityState}
            canChangeFilter={visibleTabs.length > 1}
            canLoadMore={typeof onLoadMore === "function"}
            canSync={canSync}
            config={config}
            emptyState={resolvedEmptyState}
            isLoading={isLoading}
            onMailboxCapabilityRetry={onMailboxCapabilityRetry}
            onMailboxSetup={onMailboxSetup}
            onSync={onSync}
            progressLabel={progressLabel}
          />
        )}
      </div>
    </section>
  );
}
