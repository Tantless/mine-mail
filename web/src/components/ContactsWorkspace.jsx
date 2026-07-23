import {
  AddressBook,
  ArrowLeft,
  ArrowDownLeft,
  ArrowUpRight,
  EnvelopeSimple,
  List,
  MagnifyingGlass,
  NotePencil,
  Star,
  UsersThree,
  WarningCircle,
} from "@phosphor-icons/react";
import { useState } from "react";
import { formatFullDate, formatMailTime } from "../utils/formatters.js";
import { EditableProfileAvatar, ProfileAvatar } from "./ProfileAvatar.jsx";
import "./ContactsWorkspace.css";

const contactFilters = [
  { id: "all", label: "全部" },
  { id: "favorite", label: "收藏" },
];

function contactLabel(contact) {
  return contact?.displayName?.trim() || contact?.email || "未知联系人";
}

function contactOriginalName(contact) {
  return contact?.originalName?.trim() || contact?.email || "未知联系人";
}

function contactKey(contact) {
  return contact?.email?.trim().toLowerCase() || "";
}

function errorMessage(error, fallback) {
  if (typeof error === "string" && error.trim()) return error;
  if (error?.message) return error.message;
  return fallback;
}

function messageKey(message, index) {
  return `${message.mailbox || message.kind || "mail"}:${message.uid ?? index}`;
}

function isOutgoingMessage(message) {
  return (
    message.direction === "outgoing" ||
    message.kind === "sent" ||
    message.kind === "outbox"
  );
}

function messageTime(message) {
  return message.sent_at || message.internal_date || message.synced_at || null;
}

function mailboxRoleLabel(message) {
  if (message.mailbox_role === "inbox") return "INBOX";
  if (message.mailbox_role === "sent") return "SENT";
  return "";
}

function newestFirst(messages) {
  return [...messages].sort((left, right) => {
    const leftTime = Date.parse(messageTime(left) || "");
    const rightTime = Date.parse(messageTime(right) || "");
    return (Number.isNaN(rightTime) ? 0 : rightTime) -
      (Number.isNaN(leftTime) ? 0 : leftTime);
  });
}

function ContactsLoadingState() {
  return (
    <div className="contacts-state contacts-state--loading" role="status" aria-label="正在加载联系人">
      <span className="contacts-skeleton" />
      <span className="contacts-skeleton" />
      <span className="contacts-skeleton" />
      <span className="contacts-skeleton__label">正在加载联系人…</span>
    </div>
  );
}

function ContactsListState({ error, query, onRetry }) {
  if (error) {
    return (
      <div className="contacts-state" role="alert">
        <WarningCircle size={28} weight="duotone" aria-hidden="true" />
        <strong>联系人加载失败</strong>
        <span>{errorMessage(error, "暂时无法读取本地联系人。")}</span>
        {onRetry ? (
          <button type="button" className="contacts-secondary-button" onClick={onRetry}>
            重新加载
          </button>
        ) : null}
      </div>
    );
  }

  return (
    <div className="contacts-state" role="status">
      <UsersThree size={28} weight="duotone" aria-hidden="true" />
      <strong>{query.trim() ? "没有找到联系人" : "还没有联系人"}</strong>
      <span>
        {query.trim()
          ? "换个名称或邮箱关键词试试。"
          : "收发邮件后，常用往来对象会出现在这里。"}
      </span>
    </div>
  );
}

function ContactList({ contacts, selectedContact, onSelectContact, onToggleFavorite }) {
  const selectedKey = contactKey(selectedContact);

  return (
    <div className="contacts-list" role="list" aria-label="联系人">
      {contacts.map((contact, index) => {
        const key = contactKey(contact) || `contact-${index}`;
        const selected = Boolean(selectedKey) && key === selectedKey;
        const label = contactLabel(contact);
        const messageCount = Number(contact.messageCount) || 0;

        return (
          <article
            className="contacts-row"
            data-selected={selected}
            data-favorite={Boolean(contact.isFavorite)}
            role="listitem"
            style={{ "--row-index": index }}
            key={key}
          >
            <button
              type="button"
              className="contacts-row__select"
              aria-label={`查看联系人 ${label}`}
              aria-current={selected ? "true" : undefined}
              onClick={() => onSelectContact(contact)}
            >
              <ProfileAvatar
                className="contacts-row__avatar"
                email={contact.email}
                label={label}
                customSrc={contact.avatarSrc}
              />
              <span className="contacts-row__copy">
                <span className="contacts-row__heading">
                  <strong>{label}</strong>
                  {contact.lastMessageAt ? (
                    <time
                      dateTime={contact.lastMessageAt}
                      title={formatFullDate(contact.lastMessageAt)}
                    >
                      {formatMailTime(contact.lastMessageAt)}
                    </time>
                  ) : null}
                </span>
                <span className="contacts-row__email">{contact.email}</span>
                <span className="contacts-row__meta">
                  {messageCount} 封往来
                  {contact.lastSubject ? ` · ${contact.lastSubject}` : ""}
                </span>
              </span>
            </button>
            <button
              type="button"
              className="contacts-row__favorite"
              data-active={Boolean(contact.isFavorite)}
              aria-label={contact.isFavorite ? `取消收藏 ${label}` : `收藏 ${label}`}
              aria-pressed={Boolean(contact.isFavorite)}
              title={contact.isFavorite ? "取消收藏" : "收藏联系人"}
              onClick={() => onToggleFavorite(contact)}
            >
              <Star size={18} weight={contact.isFavorite ? "fill" : "regular"} />
            </button>
          </article>
        );
      })}
    </div>
  );
}

function ContactDetails({
  contact,
  messages,
  isMessagesLoading,
  messagesError,
  onRetryMessages,
  onBackToContacts,
  onToggleFavorite,
  onCompose,
  onOpenMessage,
  onSaveRemark,
  onSetAvatar,
  onRemoveAvatar,
}) {
  if (!contact) {
    return (
      <section className="reader-panel contacts-detail-panel contacts-detail-panel--empty" aria-label="联系人详情">
        <div className="contacts-detail-empty">
          <span className="contacts-detail-empty__art" aria-hidden="true">
            <AddressBook size={34} weight="duotone" />
          </span>
          <strong>选择一个联系人</strong>
          <span>这里会显示联系人资料和往来邮件。</span>
        </div>
      </section>
    );
  }

  const label = contactLabel(contact);
  const originalName = contactOriginalName(contact);
  const sortedMessages = newestFirst(messages);

  return (
    <section className="reader-panel contacts-detail-panel" aria-label={`${label} 的联系人详情`}>
      <div className="contacts-detail-scroll vertical-scroll-surface">
        {onBackToContacts ? (
          <button
            type="button"
            className="contacts-detail-back"
            onClick={onBackToContacts}
          >
            <ArrowLeft size={18} />
            返回联系人
          </button>
        ) : null}
        <header className="contacts-profile">
          <EditableProfileAvatar
            className="contacts-profile__avatar-picker"
            avatarClassName="contacts-profile__avatar"
            email={contact.email}
            label={label}
            customSrc={contact.avatarSrc}
            onSelectFile={(file) => onSetAvatar(contact, file)}
            onRemove={() => onRemoveAvatar(contact)}
          />
          <div className="contacts-profile__identity">
            <p className="eyebrow">CONTACT</p>
            <h1>{label}</h1>
            {contact.remark ? (
              <span className="contacts-profile__original-name">({originalName})</span>
            ) : null}
            <span className="contacts-profile__email">{contact.email}</span>
            <span>{Number(contact.messageCount) || 0} 封往来邮件</span>
          </div>
          <button
            type="button"
            className="contacts-profile__favorite"
            data-active={Boolean(contact.isFavorite)}
            aria-label={contact.isFavorite ? `取消收藏 ${label}` : `收藏 ${label}`}
            aria-pressed={Boolean(contact.isFavorite)}
            onClick={() => onToggleFavorite(contact)}
          >
            <Star size={21} weight={contact.isFavorite ? "fill" : "regular"} />
          </button>
        </header>

        <div className="contacts-profile-actions" aria-label="联系人操作">
          <button
            type="button"
            className="contacts-primary-button"
            onClick={() => onCompose(contact)}
          >
            <EnvelopeSimple size={18} weight="bold" />
            写信
          </button>
          <ContactRemarkEditor
            key={contact.email}
            contact={contact}
            onSaveRemark={onSaveRemark}
          />
        </div>

        <section className="contacts-correspondence" aria-labelledby="contacts-correspondence-title">
          <div className="contacts-correspondence__heading">
            <div>
              <p className="eyebrow">CORRESPONDENCE</p>
              <h2 id="contacts-correspondence-title">往来邮件</h2>
            </div>
            <span>{sortedMessages.length} 封</span>
          </div>

          {isMessagesLoading && !messages.length ? (
            <div className="contacts-correspondence-state" role="status">
              <span className="contacts-correspondence-spinner" aria-hidden="true" />
              正在加载往来邮件…
            </div>
          ) : messagesError ? (
            <div className="contacts-correspondence-state" role="alert">
              <WarningCircle size={24} weight="duotone" aria-hidden="true" />
              <strong>往来邮件加载失败</strong>
              <span>{errorMessage(messagesError, "暂时无法读取往来记录。")}</span>
              {onRetryMessages ? (
                <button type="button" className="contacts-secondary-button" onClick={onRetryMessages}>
                  重新加载
                </button>
              ) : null}
            </div>
          ) : sortedMessages.length ? (
            <div className="contacts-message-list" role="list" aria-label={`与 ${label} 的往来邮件`}>
              {sortedMessages.map((message, index) => {
                const outgoing = isOutgoingMessage(message);
                const subject = message.subject || "（无主题）";
                const timestamp = messageTime(message);
                const mailboxLabel = mailboxRoleLabel(message);
                return (
                  <article role="listitem" key={messageKey(message, index)}>
                    <button
                      type="button"
                      className="contacts-message-row"
                      aria-label={`打开邮件：${subject}`}
                      onClick={() => onOpenMessage(message)}
                    >
                      <span className="contacts-message-row__direction" data-outgoing={outgoing} aria-hidden="true">
                        {outgoing ? <ArrowUpRight size={18} /> : <ArrowDownLeft size={18} />}
                      </span>
                      <span className="contacts-message-row__copy">
                        <span className="contacts-message-row__topline">
                          <strong>{subject}</strong>
                          {timestamp ? (
                            <time dateTime={timestamp} title={formatFullDate(timestamp)}>
                              {formatMailTime(timestamp)}
                            </time>
                          ) : null}
                        </span>
                        <span className="contacts-message-row__preview">
                          {message.preview || "暂无摘要"}
                        </span>
                        <span className="contacts-message-row__meta">
                          {outgoing ? "发给对方" : "对方发来"}
                          {mailboxLabel ? ` · ${mailboxLabel}` : ""}
                        </span>
                      </span>
                    </button>
                  </article>
                );
              })}
            </div>
          ) : (
            <div className="contacts-correspondence-state" role="status">
              <EnvelopeSimple size={26} weight="duotone" aria-hidden="true" />
              <strong>还没有往来邮件</strong>
              <span>写一封邮件，开始你们的对话。</span>
            </div>
          )}
        </section>
      </div>
    </section>
  );
}

function ContactRemarkEditor({ contact, onSaveRemark }) {
  const savedRemark = contact?.remark?.trim() || "";
  const [value, setValue] = useState(savedRemark);
  const [isOpen, setIsOpen] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState(null);

  const save = async () => {
    setIsSaving(true);
    setError(null);
    try {
      await onSaveRemark(contact, value.trim());
      setIsOpen(false);
    } catch (saveError) {
      setError(errorMessage(saveError, "备注没有保存，请重试。"));
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <form
      className="contacts-remark-editor"
      data-open={isOpen}
      aria-label="联系人备注"
      autoComplete="off"
      onSubmit={(event) => {
        event.preventDefault();
        if (!isSaving) void save();
      }}
    >
      <button
        type="button"
        className="contacts-remark-editor__toggle"
        data-active={Boolean(savedRemark)}
        aria-label={savedRemark ? "编辑备注" : "添加备注"}
        aria-expanded={isOpen}
        title={savedRemark ? `编辑备注：${savedRemark}` : "添加备注"}
        onClick={() => {
          if (isOpen) {
            setIsOpen(false);
            setValue(savedRemark);
            setError(null);
            return;
          }
          setValue(savedRemark);
          setError(null);
          setIsOpen(true);
        }}
      >
        <NotePencil size={18} weight={savedRemark ? "fill" : "regular"} />
      </button>
      {isOpen ? (
        <span className="contacts-remark-editor__fields">
          <input
            className="contacts-remark-editor__input"
            type="text"
            value={value}
            maxLength={80}
            placeholder="输入备注"
            aria-label="联系人备注名"
            autoComplete="off"
            autoFocus
            disabled={isSaving}
            onChange={(event) => {
              setValue(event.target.value);
              setError(null);
            }}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                setValue(savedRemark);
                setError(null);
                setIsOpen(false);
              }
            }}
          />
          <button
            type="submit"
            className="contacts-remark-editor__save"
            disabled={isSaving}
          >
            {isSaving ? "保存中…" : "保存"}
          </button>
        </span>
      ) : null}
      {error ? (
        <span className="contacts-remark-editor__error" role="alert">{error}</span>
      ) : null}
    </form>
  );
}

/**
 * Controlled contacts workspace. Render it directly inside `.mail-layout` so
 * its two sibling panels occupy the existing middle and reader grid columns.
 */
export function ContactsWorkspace({
  contacts = [],
  selectedContact = null,
  messages = [],
  query = "",
  filter = "all",
  isLoading = false,
  error = null,
  isMessagesLoading = false,
  messagesError = null,
  readerContent = null,
  onRetry = null,
  onRetryMessages = null,
  onBackToContacts = null,
  onOpenMobileNav = null,
  onSearchChange = () => {},
  onFilterChange = () => {},
  onSelectContact = () => {},
  onToggleFavorite = () => {},
  onCompose = () => {},
  onOpenMessage = () => {},
  onSaveRemark = () => {},
  onSetAvatar = () => {},
  onRemoveAvatar = () => {},
}) {
  return (
    <>
      <section className="mail-list-panel contacts-list-panel" aria-label="通讯录联系人列表">
        <div className="contacts-list-topbar">
          {onOpenMobileNav ? (
            <button
              type="button"
              className="mobile-nav-button"
              aria-label="打开导航"
              onClick={onOpenMobileNav}
            >
              <List size={21} />
            </button>
          ) : null}
          <label className="contacts-search inset-input-shell">
            <MagnifyingGlass size={18} aria-hidden="true" />
            <input
              value={query}
              autoComplete="off"
              onChange={(event) => onSearchChange(event.target.value)}
              placeholder="搜索名称或邮箱"
              aria-label="搜索联系人"
            />
          </label>
        </div>

        <div className="contacts-list-heading">
          <div>
            <p className="eyebrow">ADDRESS BOOK</p>
            <h1>通讯录</h1>
          </div>
          <AddressBook size={24} weight="duotone" aria-hidden="true" />
        </div>

        <div className="contacts-tabs" role="tablist" aria-label="联系人筛选">
          {contactFilters.map((item) => (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={filter === item.id}
              data-selected={filter === item.id}
              onClick={() => onFilterChange(item.id)}
            >
              {item.label}
            </button>
          ))}
          <span>{contacts.length} 人</span>
        </div>

        <div className="contacts-list-body vertical-scroll-surface">
          {isLoading && !contacts.length ? (
            <ContactsLoadingState />
          ) : error || !contacts.length ? (
            <ContactsListState error={error} query={query} onRetry={onRetry} />
          ) : (
            <ContactList
              contacts={contacts}
              selectedContact={selectedContact}
              onSelectContact={onSelectContact}
              onToggleFavorite={onToggleFavorite}
            />
          )}
        </div>
      </section>

      {readerContent ?? (
        <ContactDetails
          contact={selectedContact}
          messages={messages}
          isMessagesLoading={isMessagesLoading}
          messagesError={messagesError}
          onRetryMessages={onRetryMessages}
          onBackToContacts={onBackToContacts}
          onToggleFavorite={onToggleFavorite}
          onCompose={onCompose}
          onOpenMessage={onOpenMessage}
          onSaveRemark={onSaveRemark}
          onSetAvatar={onSetAvatar}
          onRemoveAvatar={onRemoveAvatar}
        />
      )}
    </>
  );
}
