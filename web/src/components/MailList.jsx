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

const tabs = [
  { id: "all", label: "全部" },
  { id: "unread", label: "未读" },
  { id: "starred", label: "收藏" },
];

export function MailList({
  folderLabel,
  messages,
  selectedUid,
  selectedMessage = null,
  onSelect,
  onToggleStar = null,
  query,
  onQueryChange,
  filter,
  onFilterChange,
  onSync,
  syncState,
  loadState = { phase: "ready", completed: 0, total: null },
  canSync = true,
  onOpenMobileNav,
  avatarForEmail = () => null,
  displayNameForEmail = () => null,
  referenceJump = null,
}) {
  const messageListRef = useRef(null);
  const isLoading =
    loadState.phase === "loading" || loadState.phase === "syncing";
  const isSyncing = syncState === "syncing" || loadState.phase === "syncing";
  const progressLabel =
    loadState.phase === "loading"
      ? "正在读取本地邮件…"
      : loadState.total
        ? `正在同步，已加载 ${loadState.completed}/${loadState.total} 封`
        : "正在连接邮箱并同步…";

  useEffect(() => {
    if (!referenceJump?.key || !messageListRef.current) return;
    const targetRow = Array.from(
      messageListRef.current.querySelectorAll(".mail-row"),
    ).find((row) => row.dataset.navigationKey === referenceJump.key);
    if (!targetRow) return;
    targetRow.scrollIntoView?.({ block: "nearest" });
    targetRow.focus({ preventScroll: true });
  }, [referenceJump]);

  return (
    <section className="mail-list-panel" aria-label={`${folderLabel}邮件列表`}>
      <div className="list-topbar">
        <button
          type="button"
          className="mobile-nav-button"
          aria-label="打开导航"
          onClick={onOpenMobileNav}
        >
          <List size={21} />
        </button>
        <label className="search-box inset-input-shell">
          <MagnifyingGlass size={18} aria-hidden="true" />
          <input
            value={query}
            autoComplete="off"
            onChange={(event) => onQueryChange(event.target.value)}
            placeholder="搜索邮件"
            aria-label="搜索邮件"
          />
        </label>
        <IconButton label="筛选邮件">
          <FunnelSimple size={19} />
        </IconButton>
      </div>

      <div className="list-heading">
        <div>
          <p className="eyebrow">MAILBOX</p>
          <h1>{folderLabel}</h1>
        </div>
        <IconButton
          label={isSyncing ? "正在同步" : "同步收件箱"}
          onClick={onSync}
          disabled={isSyncing || !canSync}
          className={isSyncing ? "is-spinning" : ""}
        >
          <ArrowClockwise size={19} />
        </IconButton>
      </div>

      <div className="mail-tabs" role="tablist" aria-label="邮件筛选">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={filter === tab.id}
            className="mail-tab"
            data-selected={filter === tab.id}
            onClick={() => onFilterChange(tab.id)}
          >
            {tab.label}
          </button>
        ))}
        <span className="mail-tabs__count">{messages.length} 封</span>
      </div>

      <div
        className="message-list vertical-scroll-surface"
        role="listbox"
        aria-label="邮件"
        ref={messageListRef}
      >
        {isLoading && messages.length ? (
          <div className="mail-load-progress" role="status">
            <CircleNotch size={15} aria-hidden="true" />
            <span>{progressLabel}</span>
          </div>
        ) : null}
        {messages.length ? (
          messages.map((message, index) => {
            const navigationKey = messageNavigationKey(message);
            const selectedNavigationKey = messageNavigationKey(selectedMessage);
            const selected =
              navigationKey && selectedNavigationKey
                ? navigationKey === selectedNavigationKey
                : message.uid === selectedUid;
            const unread = !hasFlag(message, "\\Seen");
            const starred = hasFlag(message, "\\Flagged");
            const sender =
              displayNameForEmail(message.sender?.email)?.trim() ||
              senderLabel(message);
            const canToggleStar =
              typeof onToggleStar === "function" &&
              message.kind !== "draft" &&
              message.kind !== "outbox";
            const subject = message.subject || "（无主题）";

            return (
              <article
                key={navigationKey || message.uid}
                className="mail-row"
                data-selected={selected}
                data-unread={unread}
                data-navigation-key={navigationKey || undefined}
                role="option"
                aria-selected={selected}
                tabIndex={0}
                style={{ "--row-index": index }}
                onClick={() => onSelect(message)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    onSelect(message);
                  }
                }}
              >
                <ProfileAvatar
                  className="mail-row__avatar"
                  email={message.sender?.email}
                  label={sender}
                  customSrc={avatarForEmail(message.sender?.email)}
                />
                <span className="mail-row__content">
                  <span className="mail-row__meta">
                    <span className="mail-row__sender">
                      {unread ? <span className="unread-dot" /> : null}
                      {sender}
                    </span>
                    <time dateTime={message.sent_at}>{formatMailTime(message.sent_at)}</time>
                  </span>
                  <span className="mail-row__subject">{message.subject || "（无主题）"}</span>
                  <span className="mail-row__preview">{message.preview || "暂无摘要"}</span>
                </span>
                <TooltipTarget label={starred ? "取消收藏" : "添加收藏"}>
                  <button
                    type="button"
                    className="star-button"
                    data-active={starred}
                    aria-label={starred ? `取消收藏：${subject}` : `添加收藏：${subject}`}
                    aria-pressed={starred}
                    disabled={!canToggleStar}
                    onClick={(event) => {
                      event.stopPropagation();
                      if (canToggleStar) onToggleStar(message);
                    }}
                    onKeyDown={(event) => event.stopPropagation()}
                  >
                    <Star size={17} weight={starred ? "fill" : "regular"} />
                  </button>
                </TooltipTarget>
              </article>
            );
          })
        ) : isLoading ? (
          <div className="mail-loading-state" role="status">
            <CircleNotch size={24} aria-hidden="true" />
            <strong>{progressLabel}</strong>
            <span>邮件会分批显示，无需等待全部同步完成</span>
            <div className="mail-loading-skeletons" aria-hidden="true">
              {[0, 1, 2].map((index) => (
                <span className="mail-loading-skeleton" key={index}>
                  <i />
                  <b />
                </span>
              ))}
            </div>
          </div>
        ) : loadState.phase === "error" ? (
          <div className="empty-list" role="alert">
            <ArrowClockwise size={26} />
            <strong>部分邮件暂时没有加载完成</strong>
            <span>可以点击右上角的同步按钮重试</span>
          </div>
        ) : (
          <div className="empty-list">
            <MagnifyingGlass size={26} />
            <strong>没有找到邮件</strong>
            <span>换个关键词或筛选条件试试</span>
          </div>
        )}
      </div>

    </section>
  );
}
