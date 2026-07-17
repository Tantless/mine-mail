import { useCallback, useEffect, useRef, useState } from "react";
import { EnvelopeSimple, X } from "@phosphor-icons/react";
import { mailApi } from "../services/mailApi.js";

const visibleDurationMs = 8000;
const validThemes = new Set(["daylight", "night", "dusk", "forest"]);

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
  const dismissTimerRef = useRef(null);
  const lastPresentedIdRef = useRef(0);

  const clearDismissTimer = useCallback(() => {
    if (dismissTimerRef.current !== null) {
      window.clearTimeout(dismissTimerRef.current);
      dismissTimerRef.current = null;
    }
  }, []);

  const dismiss = useCallback(
    async (item) => {
      if (!item) return;
      clearDismissTimer();
      setNotification((current) =>
        current?.notificationId === item.notificationId ? null : current,
      );
      try {
        await mailApi.dismissNewMailNotification(item.notificationId);
      } catch {
        // The surface is transient. A later notification or app exit will
        // reconcile it even if the native window is already disappearing.
      }
    },
    [clearDismissTimer],
  );

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
      if (!item || item.notificationId <= lastPresentedIdRef.current) return;
      lastPresentedIdRef.current = item.notificationId;
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
    setNotification(null);
    try {
      if (current.accountId) {
        await mailApi.openNewMailNotification(
          current.notificationId,
          current.uid,
          current.accountId,
        );
      } else {
        await mailApi.openNewMailNotification(current.notificationId, current.uid);
      }
    } catch {
      setNotification(current);
      scheduleDismiss(current);
    }
  };

  if (!notification) return null;

  return (
    <article
      className="new-mail-notification"
      aria-label={`${notification.sender}：${notification.subject}`}
      onMouseEnter={clearDismissTimer}
      onMouseLeave={() => scheduleDismiss(notification)}
    >
      <button
        type="button"
        className="new-mail-notification__main"
        aria-label="打开新邮件"
        onClick={openMessage}
      >
        <span className="new-mail-notification__icon" aria-hidden="true">
          <EnvelopeSimple size={25} weight="duotone" />
        </span>
        <span className="new-mail-notification__copy">
          <span className="new-mail-notification__eyebrow">
            <span className="new-mail-notification__dot" />
            MINE MAIL · 刚刚
          </span>
          <strong>{notification.sender}</strong>
          <span>{notification.subject}</span>
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
