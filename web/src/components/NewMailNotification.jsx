import { useCallback, useEffect, useRef, useState } from "react";
import { X } from "@phosphor-icons/react";
import { mailApi } from "../services/mailApi.js";
import { playWebNotificationSound } from "../utils/notificationSound.js";
import { ProfileAvatar } from "./ProfileAvatar.jsx";
import {
  applyAppearancePaletteToRoot,
  appearanceFromSavedAppearance,
} from "../appearanceThemes.js";

const visibleDurationMs = 8000;
const notificationCountLimit = 99;
const validThemes = new Set(["daylight", "night", "dusk", "forest"]);
const MAX_DISMISS_RETRIES = 3;

function notificationSequence(value) {
  if (typeof value === "bigint") return value >= 0n ? value : null;
  if (typeof value === "string" && /^\d+$/.test(value.trim())) {
    return BigInt(value.trim());
  }
  if (
    typeof value === "number" &&
    Number.isFinite(value) &&
    Number.isInteger(value) &&
    value >= 0
  ) {
    return BigInt(value);
  }
  return null;
}

function sameNotification(left, right) {
  const leftSequence = notificationSequence(left?.notificationId);
  const rightSequence = notificationSequence(right?.notificationId);
  return (
    leftSequence !== null &&
    rightSequence !== null &&
    leftSequence === rightSequence
  );
}

function notificationCountLabel(count) {
  if (count <= 1) return "新邮件 · 刚刚";
  const displayedCount =
    count > notificationCountLimit ? `${notificationCountLimit}+` : count;
  return `${displayedCount} 封新邮件 · 刚刚`;
}

function applySavedTheme() {
  const saved = window.localStorage.getItem("mine-mail-theme");
  const root = document.documentElement;
  const appearance = appearanceFromSavedAppearance();
  root.dataset.theme =
    saved === "custom" ? "custom" : validThemes.has(saved) ? saved : "daylight";
  root.dataset.appearanceMode = appearance.minimalModeEnabled
    ? "minimal"
    : "image";
  delete root.dataset.colorMode;
  applyAppearancePaletteToRoot(root, appearance.paletteId);
}

export function NewMailNotification() {
  const [notification, setNotification] = useState(null);
  const notificationRef = useRef(null);
  const dismissActionRef = useRef(null);
  const dismissTimerRef = useRef(null);
  const lastPresentedIdRef = useRef(null);
  // Bounded retries for the dismiss command so a persistently failing backend
  // cannot trigger an endless restore-and-retry loop every few seconds.
  const dismissRetryCountRef = useRef(0);
  const dismissRetryNotificationIdRef = useRef(null);

  const resetDismissRetriesFor = useCallback((item) => {
    const sequence = notificationSequence(item?.notificationId);
    if (sequence === null || sequence === dismissRetryNotificationIdRef.current) {
      return;
    }
    dismissRetryNotificationIdRef.current = sequence;
    dismissRetryCountRef.current = 0;
  }, []);

  const clearDismissTimer = useCallback(() => {
    if (dismissTimerRef.current !== null) {
      window.clearTimeout(dismissTimerRef.current);
      dismissTimerRef.current = null;
    }
  }, []);

  const hideNotification = useCallback((item) => {
    setNotification((current) => {
      if (!sameNotification(current, item)) return current;
      notificationRef.current = null;
      return null;
    });
  }, []);

  const restoreNewestNotification = useCallback((...items) => {
    const candidates = [notificationRef.current, ...items].filter(Boolean);
    if (!candidates.length) return;
    const newest = candidates.reduce((current, candidate) => {
      const currentSequence = notificationSequence(current.notificationId);
      const candidateSequence = notificationSequence(candidate.notificationId);
      return candidateSequence !== null &&
        (currentSequence === null || candidateSequence > currentSequence)
        ? candidate
        : current;
    });
    const newestSequence = notificationSequence(newest.notificationId);
    if (
      newestSequence !== null &&
      lastPresentedIdRef.current !== null &&
      newestSequence < lastPresentedIdRef.current
    ) {
      return;
    }
    if (
      newestSequence !== null &&
      (lastPresentedIdRef.current === null ||
        newestSequence > lastPresentedIdRef.current)
    ) {
      lastPresentedIdRef.current = newestSequence;
    }
    resetDismissRetriesFor(newest);
    notificationRef.current = newest;
    setNotification(newest);
    clearDismissTimer();
    dismissTimerRef.current = window.setTimeout(
      () => void dismissActionRef.current?.(newest),
      visibleDurationMs,
    );
  }, [clearDismissTimer, resetDismissRetriesFor]);

  const dismiss = useCallback(
    async (item) => {
      if (!item) return;
      clearDismissTimer();
      hideNotification(item);
      const canRetry = dismissRetryCountRef.current < MAX_DISMISS_RETRIES;
      try {
        const dismissed = await mailApi.dismissNewMailNotification(
          item.notificationId,
        );
        if (dismissed === true) {
          dismissRetryCountRef.current = 0;
          dismissRetryNotificationIdRef.current = null;
        } else {
          const pending = await mailApi.getNewMailNotification().catch(() => null);
          if (pending && canRetry) {
            dismissRetryCountRef.current += 1;
            restoreNewestNotification(pending);
          }
        }
      } catch {
        try {
          const pending = await mailApi.getNewMailNotification();
          if (pending && canRetry) {
            dismissRetryCountRef.current += 1;
            restoreNewestNotification(pending);
          }
        } catch {
          if (canRetry) {
            dismissRetryCountRef.current += 1;
            restoreNewestNotification(item);
          }
        }
      }
    },
    [clearDismissTimer, hideNotification, restoreNewestNotification],
  );
  dismissActionRef.current = dismiss;

  const scheduleDismiss = useCallback(
    (item) => {
      clearDismissTimer();
      dismissTimerRef.current = window.setTimeout(
        () => void dismiss(item),
        visibleDurationMs,
      );
    },
    [clearDismissTimer, dismiss],
  );

  const present = useCallback(
    (item) => {
      const sequence = notificationSequence(item?.notificationId);
      if (
        !item ||
        sequence === null ||
        (lastPresentedIdRef.current !== null &&
          sequence <= lastPresentedIdRef.current)
      ) {
        return;
      }
      lastPresentedIdRef.current = sequence;
      resetDismissRetriesFor(item);
      notificationRef.current = item;
      setNotification(item);
      void playWebNotificationSound(item.webSound).catch(() => {});
      scheduleDismiss(item);
    },
    [resetDismissRetriesFor, scheduleDismiss],
  );

  useEffect(() => {
    applySavedTheme();
    const handleStorage = (event) => {
      if (event.key?.startsWith("mine-mail-")) applySavedTheme();
    };
    window.addEventListener("storage", handleStorage);
    let cancelled = false;
    let unlisten = null;
    const connect = async () => {
      const dispose = await mailApi.onMailEvent(
        "mail:new-mail-notification",
        (event) => present(event?.payload),
      );
      if (cancelled) {
        dispose();
        return;
      }
      unlisten = dispose;
      const pending = await mailApi.getNewMailNotification();
      if (!cancelled) present(pending);
    };
    void connect().catch(() => {});
    return () => {
      cancelled = true;
      clearDismissTimer();
      unlisten?.();
      window.removeEventListener("storage", handleStorage);
    };
  }, [clearDismissTimer, present]);

  const openMessage = async () => {
    if (!notification) return;
    clearDismissTimer();
    const current = notification;
    hideNotification(current);
    try {
      const opened = await mailApi.openNewMailNotification(current.notificationId);
      if (opened !== true) {
        const pending = await mailApi.getNewMailNotification().catch(() => null);
        if (pending) restoreNewestNotification(pending);
      }
    } catch {
      try {
        const pending = await mailApi.getNewMailNotification();
        if (pending) restoreNewestNotification(pending);
      } catch {
        restoreNewestNotification(current);
      }
    }
  };

  if (!notification) return null;
  const senderEmail = notification.senderEmail || "";
  const senderLabel =
    notification.senderRemark ||
    notification.sender ||
    senderEmail ||
    "未知发件人";
  const recipientEmail = notification.recipientEmail || "";
  const recipientLabel = notification.recipientRemark
    ? `${notification.recipientRemark} · ${recipientEmail}`
    : recipientEmail;

  return (
    <article
      className="new-mail-notification"
      aria-label={`${senderLabel}${
        senderEmail ? `，${senderEmail}` : ""
      }：${notification.subject}${
        recipientLabel ? `；收信至 ${recipientLabel}` : ""
      }`}
      onMouseEnter={clearDismissTimer}
      onMouseLeave={() => scheduleDismiss(notification)}
    >
      <button
        type="button"
        className="new-mail-notification__main"
        aria-label="打开新邮件"
        onClick={openMessage}
      >
        <ProfileAvatar
          className="new-mail-notification__avatar"
          email={senderEmail}
          label={senderLabel}
          customSrc={notification.senderAvatarDataUrl}
        />
        <span className="new-mail-notification__copy">
          <span className="new-mail-notification__eyebrow">
            <span className="new-mail-notification__dot" />
            {notificationCountLabel(notification.count)}
          </span>
          <span className="new-mail-notification__sender">
            <strong>{senderLabel}</strong>
            {senderEmail ? <span>{senderEmail}</span> : null}
          </span>
          <span className="new-mail-notification__subject">
            {notification.subject}
          </span>
          {recipientLabel ? (
            <span className="new-mail-notification__recipient">
              收信至 {recipientLabel}
            </span>
          ) : null}
        </span>
      </button>
      <button
        type="button"
        className="new-mail-notification__close"
        aria-label="关闭新邮件通知"
        onClick={(event) => {
          event.stopPropagation();
          void dismiss(notification);
        }}
      >
        <X size={17} weight="bold" />
      </button>
    </article>
  );
}
