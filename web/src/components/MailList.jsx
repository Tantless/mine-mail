import {
  ArrowClockwise,
  FunnelSimple,
  List,
  MagnifyingGlass,
  Star,
} from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { ProfileAvatar } from "./ProfileAvatar.jsx";
import {
  formatMailTime,
  hasFlag,
  senderLabel,
} from "../utils/formatters.js";

const tabs = [
  { id: "all", label: "全部" },
  { id: "unread", label: "未读" },
  { id: "starred", label: "星标" },
];

export function MailList({
  folderLabel,
  messages,
  selectedUid,
  onSelect,
  query,
  onQueryChange,
  filter,
  onFilterChange,
  onSync,
  syncState,
  canSync = true,
  onOpenMobileNav,
  avatarForEmail = () => null,
}) {
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
          label={syncState === "syncing" ? "正在同步" : "同步收件箱"}
          onClick={onSync}
          disabled={syncState === "syncing" || !canSync}
          className={syncState === "syncing" ? "is-spinning" : ""}
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

      <div className="message-list" role="listbox" aria-label="邮件">
        {messages.length ? (
          messages.map((message, index) => {
            const selected = message.uid === selectedUid;
            const unread = !hasFlag(message, "\\Seen");
            const starred = hasFlag(message, "\\Flagged");
            const sender = senderLabel(message);

            return (
              <article
                key={message.uid}
                className="mail-row"
                data-selected={selected}
                data-unread={unread}
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
                <button
                  type="button"
                  className="star-button"
                  data-active={starred}
                  aria-label="星标（尚未实现）"
                  title="星标操作尚未实现"
                  disabled
                >
                  <Star size={17} weight={starred ? "fill" : "regular"} />
                </button>
              </article>
            );
          })
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
