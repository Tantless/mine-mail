import { Star, X } from "@phosphor-icons/react";
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { normalizeAvatarEmail, ProfileAvatar } from "./ProfileAvatar.jsx";
import { splitAddresses } from "../utils/formatters.js";

const defaultSuggestionLimit = 5;
const popupMargin = 8;
const popupGap = 7;
const popupEntryTrackingMs = 320;
const emailPattern = /^[^\s@<>]+@[^\s@<>]+\.[^\s@<>]+$/;

function clamp(value, minimum, maximum) {
  return Math.min(Math.max(value, minimum), Math.max(minimum, maximum));
}

function contactLabel(contact) {
  return contact?.displayName?.trim() || contact?.email || "未知联系人";
}

export function isCompleteRecipientEmail(value) {
  return emailPattern.test(value.trim());
}

function matchRank(contact, query) {
  if (!query) return 0;
  const fields = [
    contactLabel(contact),
    contact.originalName,
    contact.remark,
    contact.email,
  ]
    .filter(Boolean)
    .map((value) => value.toLowerCase());
  if (fields.some((value) => value === query)) return 0;
  if (fields.some((value) => value.startsWith(query))) return 1;
  if (fields.some((value) => value.split(/[\s._@-]+/).some((part) => part.startsWith(query)))) {
    return 2;
  }
  return fields.some((value) => value.includes(query)) ? 3 : Number.POSITIVE_INFINITY;
}

export function rankRecipientContacts(contacts, query = "", selectedEmails = []) {
  const normalizedQuery = query.trim().toLowerCase();
  const selected = new Set(selectedEmails.map(normalizeAvatarEmail));
  return contacts
    .filter((contact) => contact?.email && !selected.has(normalizeAvatarEmail(contact.email)))
    .map((contact) => ({ contact, rank: matchRank(contact, normalizedQuery) }))
    .filter((item) => Number.isFinite(item.rank))
    .sort((left, right) => {
      const favoriteOrder =
        Number(Boolean(right.contact.isFavorite)) - Number(Boolean(left.contact.isFavorite));
      if (favoriteOrder) return favoriteOrder;
      if (left.rank !== right.rank) return left.rank - right.rank;
      const rightTime = Date.parse(right.contact.lastMessageAt || "") || 0;
      const leftTime = Date.parse(left.contact.lastMessageAt || "") || 0;
      if (rightTime !== leftTime) return rightTime - leftTime;
      const messageOrder =
        Number(right.contact.messageCount || 0) - Number(left.contact.messageCount || 0);
      if (messageOrder) return messageOrder;
      return contactLabel(left.contact).localeCompare(contactLabel(right.contact), "zh-CN");
    })
    .map((item) => item.contact);
}

function uniqueRecipientEmails(values) {
  const seen = new Set();
  return values.reduce((result, value) => {
    const email = value.trim();
    const key = normalizeAvatarEmail(email);
    if (!key || seen.has(key)) return result;
    seen.add(key);
    result.push(email);
    return result;
  }, []);
}

export function RecipientInput({
  id,
  label,
  recipients = [],
  contacts = [],
  onChange,
  disabled = false,
  autoFocus = false,
  placeholder = "name@example.com",
}) {
  const popupId = `recipient-suggestions-${useId().replaceAll(":", "")}`;
  const anchorRef = useRef(null);
  const inputRef = useRef(null);
  const popupRef = useRef(null);
  const closeTimerRef = useRef(null);
  const entryTrackingDeadlineRef = useRef(0);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const [popupPosition, setPopupPosition] = useState(null);

  const normalizedRecipients = useMemo(
    () => uniqueRecipientEmails(recipients),
    [recipients],
  );
  const contactsByEmail = useMemo(
    () =>
      new Map(
        contacts
          .filter((contact) => contact?.email)
          .map((contact) => [normalizeAvatarEmail(contact.email), contact]),
      ),
    [contacts],
  );
  const suggestions = useMemo(
    () => rankRecipientContacts(contacts, query, normalizedRecipients),
    [contacts, normalizedRecipients, query],
  );
  const visibleSuggestions = expanded
    ? suggestions
    : suggestions.slice(0, defaultSuggestionLimit);
  const hiddenSuggestionCount = Math.max(
    0,
    suggestions.length - defaultSuggestionLimit,
  );

  const cancelScheduledClose = useCallback(() => {
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
  }, []);

  const closeSuggestions = useCallback(() => {
    cancelScheduledClose();
    entryTrackingDeadlineRef.current = 0;
    setOpen(false);
    setExpanded(false);
    setPopupPosition(null);
  }, [cancelScheduledClose]);

  const openSuggestions = useCallback(
    (trackEntry = false) => {
      cancelScheduledClose();
      if (trackEntry) {
        entryTrackingDeadlineRef.current =
          window.performance.now() + popupEntryTrackingMs;
      }
      setOpen(true);
    },
    [cancelScheduledClose],
  );

  const scheduleClose = useCallback(() => {
    cancelScheduledClose();
    closeTimerRef.current = window.setTimeout(() => {
      closeTimerRef.current = null;
      const focused = document.activeElement;
      if (
        !anchorRef.current?.contains(focused) &&
        !popupRef.current?.contains(focused)
      ) {
        closeSuggestions();
      }
    }, 0);
  }, [cancelScheduledClose, closeSuggestions]);

  useEffect(() => () => cancelScheduledClose(), [cancelScheduledClose]);

  useEffect(() => {
    setActiveIndex(suggestions.length ? 0 : -1);
  }, [query, suggestions.length]);

  useEffect(() => {
    if (!disabled) return;
    closeSuggestions();
  }, [closeSuggestions, disabled]);

  useEffect(() => {
    if (!open) return undefined;
    const closeOnOutsidePointer = (event) => {
      if (
        anchorRef.current?.contains(event.target) ||
        popupRef.current?.contains(event.target)
      ) {
        return;
      }
      closeSuggestions();
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [closeSuggestions, open]);

  useLayoutEffect(() => {
    if (!open) return undefined;
    const anchor = anchorRef.current;
    const popup = popupRef.current;
    if (!anchor || !popup) return undefined;
    let trackingFrame = null;

    const placePopup = () => {
      const anchorRect = anchor.getBoundingClientRect();
      const popupRect = popup.getBoundingClientRect();
      const width = Math.min(
        Math.max(anchorRect.width, 320),
        window.innerWidth - popupMargin * 2,
      );
      const roomBelow = window.innerHeight - anchorRect.bottom - popupGap;
      const roomAbove = anchorRect.top - popupGap;
      const placement = roomBelow >= popupRect.height || roomBelow >= roomAbove ? "bottom" : "top";
      const top =
        placement === "bottom"
          ? anchorRect.bottom + popupGap
          : anchorRect.top - popupRect.height - popupGap;
      setPopupPosition({
        left: clamp(anchorRect.left, popupMargin, window.innerWidth - width - popupMargin),
        placement,
        top: clamp(
          top,
          popupMargin,
          window.innerHeight - popupRect.height - popupMargin,
        ),
        width,
      });
    };

    placePopup();
    const trackMovingAnchor = (timestamp) => {
      placePopup();
      if (timestamp < entryTrackingDeadlineRef.current) {
        trackingFrame = window.requestAnimationFrame(trackMovingAnchor);
      } else {
        entryTrackingDeadlineRef.current = 0;
      }
    };
    if (
      entryTrackingDeadlineRef.current > 0 &&
      typeof window.requestAnimationFrame === "function"
    ) {
      trackingFrame = window.requestAnimationFrame(trackMovingAnchor);
    }

    const motionHost = anchor.closest(".compose-panel");
    motionHost?.addEventListener("animationend", placePopup);
    motionHost?.addEventListener("transitionend", placePopup);
    window.addEventListener("resize", placePopup);
    window.addEventListener("scroll", placePopup, true);
    return () => {
      if (trackingFrame !== null && typeof window.cancelAnimationFrame === "function") {
        window.cancelAnimationFrame(trackingFrame);
      }
      motionHost?.removeEventListener("animationend", placePopup);
      motionHost?.removeEventListener("transitionend", placePopup);
      window.removeEventListener("resize", placePopup);
      window.removeEventListener("scroll", placePopup, true);
    };
  }, [expanded, open, visibleSuggestions.length]);

  const updateRecipients = (nextRecipients) => {
    onChange(uniqueRecipientEmails(nextRecipients));
  };

  const addRecipients = (values) => {
    const valid = values.filter(isCompleteRecipientEmail);
    if (!valid.length) return false;
    updateRecipients([...normalizedRecipients, ...valid]);
    return true;
  };

  const commitQuery = () => {
    const value = query.trim();
    if (!isCompleteRecipientEmail(value)) return false;
    addRecipients([value]);
    setQuery("");
    setExpanded(false);
    return true;
  };

  const selectContact = (contact) => {
    if (!contact?.email) return;
    addRecipients([contact.email]);
    setQuery("");
    setExpanded(false);
    openSuggestions();
    if (window.requestAnimationFrame) {
      window.requestAnimationFrame(() => inputRef.current?.focus());
    } else {
      inputRef.current?.focus();
    }
  };

  const handleInputChange = (event) => {
    const nextQuery = event.target.value;
    openSuggestions();
    setExpanded(false);
    if (/[;,，；\n]$/.test(nextQuery)) {
      const parts = splitAddresses(nextQuery);
      const valid = parts.filter(isCompleteRecipientEmail);
      const invalid = parts.filter((value) => !isCompleteRecipientEmail(value));
      if (valid.length) addRecipients(valid);
      setQuery(invalid.join(", "));
      return;
    }
    setQuery(nextQuery);
  };

  const handleKeyDown = (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      closeSuggestions();
      return;
    }
    if (event.key === "Backspace" && !query && normalizedRecipients.length) {
      updateRecipients(normalizedRecipients.slice(0, -1));
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      openSuggestions();
      if (!visibleSuggestions.length) return;
      const direction = event.key === "ArrowDown" ? 1 : -1;
      if (
        direction > 0 &&
        !expanded &&
        hiddenSuggestionCount > 0 &&
        activeIndex === visibleSuggestions.length - 1
      ) {
        setExpanded(true);
        setActiveIndex(defaultSuggestionLimit);
        return;
      }
      setActiveIndex((current) => {
        const start = current < 0 ? 0 : current;
        return (start + direction + visibleSuggestions.length) % visibleSuggestions.length;
      });
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      if (open && visibleSuggestions[activeIndex]) {
        selectContact(visibleSuggestions[activeIndex]);
      } else {
        commitQuery();
      }
      return;
    }
    if (event.key === "Tab") commitQuery();
  };

  const popup = open && typeof document !== "undefined"
    ? createPortal(
        <div
          ref={popupRef}
          className="recipient-suggestions"
          data-placement={popupPosition?.placement || "bottom"}
          data-ready={Boolean(popupPosition)}
          style={{
            left: popupPosition?.left ?? 0,
            top: popupPosition?.top ?? 0,
            width: popupPosition?.width ?? 320,
          }}
          onFocus={cancelScheduledClose}
          onBlur={scheduleClose}
        >
          <div className="recipient-suggestions__heading">
            <span>{query.trim() ? "匹配联系人" : "通讯录联系人"}</span>
            <small>{suggestions.length} 位</small>
          </div>
          {visibleSuggestions.length ? (
            <div
              id={popupId}
              className="recipient-suggestions__list vertical-scroll-surface"
              data-expanded={expanded}
              role="listbox"
              aria-label={`${label}联系人建议`}
            >
              {visibleSuggestions.map((contact, index) => {
                const optionId = `${popupId}-option-${index}`;
                const displayName = contactLabel(contact);
                return (
                  <button
                    id={optionId}
                    key={normalizeAvatarEmail(contact.email)}
                    type="button"
                    className="recipient-suggestion"
                    data-active={index === activeIndex}
                    data-favorite={Boolean(contact.isFavorite)}
                    role="option"
                    aria-selected={index === activeIndex}
                    tabIndex={-1}
                    onPointerEnter={() => setActiveIndex(index)}
                    onPointerDown={(event) => event.preventDefault()}
                    onClick={() => selectContact(contact)}
                  >
                    <ProfileAvatar
                      className="recipient-suggestion__avatar"
                      email={contact.email}
                      label={displayName}
                      customSrc={contact.avatarSrc}
                    />
                    <span className="recipient-suggestion__copy">
                      <strong>{displayName}</strong>
                      <small>{contact.email}</small>
                    </span>
                    {contact.isFavorite ? (
                      <span className="recipient-suggestion__favorite">
                        <Star size={12} weight="fill" aria-hidden="true" />
                        收藏
                      </span>
                    ) : null}
                  </button>
                );
              })}
            </div>
          ) : (
            <div id={popupId} className="recipient-suggestions__empty" role="status">
              {isCompleteRecipientEmail(query)
                ? "按 Enter 添加这个邮箱"
                : "没有匹配的联系人"}
            </div>
          )}
          {!expanded && hiddenSuggestionCount > 0 ? (
            <button
              type="button"
              className="recipient-suggestions__more"
              onPointerDown={(event) => event.preventDefault()}
              onClick={() => {
                setExpanded(true);
                setActiveIndex(0);
              }}
            >
              显示更多联系人
              <span>还有 {hiddenSuggestionCount} 位</span>
            </button>
          ) : null}
        </div>,
        document.body,
      )
    : null;

  return (
    <>
      <div
        ref={anchorRef}
        className="compose-input-shell inset-input-shell recipient-input"
        data-open={open}
        data-has-recipients={Boolean(normalizedRecipients.length)}
        onPointerDown={(event) => {
          if (
            !disabled &&
            !event.target.closest("button, input")
          ) {
            inputRef.current?.focus();
          }
        }}
      >
        {normalizedRecipients.map((email) => {
          const contact = contactsByEmail.get(normalizeAvatarEmail(email));
          const displayName = contactLabel(contact || { email });
          return (
            <span className="recipient-token" key={normalizeAvatarEmail(email)}>
              <ProfileAvatar
                className="recipient-token__avatar"
                email={email}
                label={displayName}
                customSrc={contact?.avatarSrc}
              />
              <span className="recipient-token__email">{email}</span>
              {!disabled ? (
                <button
                  type="button"
                  className="recipient-token__remove"
                  aria-label={`移除${label} ${email}`}
                  onClick={() =>
                    updateRecipients(
                      normalizedRecipients.filter(
                        (recipient) => normalizeAvatarEmail(recipient) !== normalizeAvatarEmail(email),
                      ),
                    )
                  }
                >
                  <X size={10} weight="bold" aria-hidden="true" />
                </button>
              ) : null}
            </span>
          );
        })}
        <input
          ref={inputRef}
          id={id}
          type="text"
          inputMode="email"
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="none"
          spellCheck={false}
          autoFocus={autoFocus}
          disabled={disabled}
          value={query}
          placeholder={normalizedRecipients.length ? "" : placeholder}
          aria-label={label}
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={open}
          aria-controls={open ? popupId : undefined}
          aria-activedescendant={
            open && visibleSuggestions[activeIndex]
              ? `${popupId}-option-${activeIndex}`
              : undefined
          }
          onFocus={() => {
            if (!disabled) openSuggestions(true);
          }}
          onPointerDown={() => {
            if (!disabled && !open) openSuggestions();
          }}
          onBlur={() => {
            commitQuery();
            scheduleClose();
          }}
          onChange={handleInputChange}
          onKeyDown={handleKeyDown}
          onPaste={(event) => {
            const pasted = event.clipboardData.getData("text");
            if (!/[;,，；\n]/.test(pasted)) return;
            event.preventDefault();
            const values = splitAddresses(pasted);
            const valid = values.filter(isCompleteRecipientEmail);
            const invalid = values.filter((value) => !isCompleteRecipientEmail(value));
            if (valid.length) addRecipients(valid);
            setQuery(invalid.join(", "));
          }}
        />
      </div>
      {popup}
    </>
  );
}
