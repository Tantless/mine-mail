import { useCallback, useEffect, useRef, useState } from "react";
import { X } from "@phosphor-icons/react";
import { mailApi } from "../services/mailApi.js";
import { ProfileAvatar } from "./ProfileAvatar.jsx";

const visibleDurationMs = 8000;
const validThemes = new Set(["daylight", "night", "dusk", "forest"]);

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

function applySavedTheme() {
  const saved = window.localStorage.getItem("mine-mail-theme");
  document.documentElement.dataset.theme = validThemes.has(saved)
    ? saved
    : "daylight";
}

function playWebSound(preset) {
  if (!preset) return;
  const AudioContext = window.AudioContext || window.webkitAudioContext;
  if (!AudioContext) return;
  const patterns = {
    default: [
      [740, 0, 0.14],
    ],
    mail: [
      [660, 0, 0.13],
      [880, 0.14, 0.18],
    ],
    im: [
      [784, 0, 0.1],
      [1047, 0.11, 0.12],
    ],
    reminder: [
      [523, 0, 0.14],
      [659, 0.16, 0.14],
      [784, 0.32, 0.2],
    ],
  };
  const context = new AudioContext();
  const startedAt = context.currentTime + 0.02;
  for (const [frequency, offset, duration] of patterns[preset] || patterns.mail) {
    const oscillator = context.createOscillator();
    const gain = context.createGain();
    oscillator.type = "sine";
    oscillator.frequency.value = frequency;
    gain.gain.setValueAtTime(0.0001, startedAt + offset);
    gain.gain.exponentialRampToValueAtTime(0.13, startedAt + offset + 0.018);
    gain.gain.exponentialRampToValueAtTime(
      0.0001,
      startedAt + offset + duration,
    );
    oscillator.connect(gain).connect(context.destination);
    oscillator.start(startedAt + offset);
    oscillator.stop(startedAt + offset + duration);
  }
  const totalDuration = Math.max(
    ...((patterns[preset] || patterns.mail).map(
      ([, offset, duration]) => offset + duration,
    )),
  );
  window.setTimeout(() => void context.close(), (totalDuration + 0.2) * 1000);
}

export function NewMailNotification() {
  const [notification, setNotification] = useState(null);
  const notificationRef = useRef(null);
  const dismissActionRef = useRef(null);
  const dismissTimerRef = useRef(null);
  const lastPresentedIdRef = useRef(null);

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
      (lastPresentedIdRef.current === null ||
        newestSequence > lastPresentedIdRef.current)
    ) {
      lastPresentedIdRef.current = newestSequence;
    }
    notificationRef.current = newest;
    setNotification(newest);
    clearDismissTimer();
    dismissTimerRef.current = window.setTimeout(
      () => void dismissActionRef.current?.(newest),
      visibleDurationMs,
    );
  }, [clearDismissTimer]);

  const dismiss = useCallback(
    async (item) => {
      if (!item) return;
      clearDismissTimer();
      hideNotification(item);
      try {
        const dismissed = await mailApi.dismissNewMailNotification(
          item.notificationId,
        );
        if (dismissed !== true) {
          const pending = await mailApi.getNewMailNotification().catch(() => null);
          if (pending) restoreNewestNotification(pending);
        }
      } catch {
        try {
          const pending = await mailApi.getNewMailNotification();
          if (pending) restoreNewestNotification(pending);
        } catch {
          restoreNewestNotification(item);
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
      notificationRef.current = item;
      setNotification(item);
      playWebSound(item.webSound);
      scheduleDismiss(item);
    },
    [scheduleDismiss],
  );

  useEffect(() => {
    applySavedTheme();
    const handleStorage = (event) => {
      if (event.key === "mine-mail-theme") applySavedTheme();
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
            {notification.count > 1
              ? `${notification.count} 封新邮件 · 刚刚`
              : "新邮件 · 刚刚"}
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
