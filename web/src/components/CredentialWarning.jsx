import { useCallback, useEffect, useRef, useState } from "react";
import { WarningCircle } from "@phosphor-icons/react";
import { Tooltip } from "./Tooltip.jsx";

const TOOLTIP_DELAY_MS = 380;
const HELP_TEXT = "需要重新登录，或从邮箱服务商处获取新的授权凭证。";

export function CredentialWarning({ compact = false }) {
  const anchorRef = useRef(null);
  const openTimerRef = useRef(null);
  const [tooltipOpen, setTooltipOpen] = useState(false);

  const cancelPendingOpen = useCallback(() => {
    if (openTimerRef.current !== null) {
      window.clearTimeout(openTimerRef.current);
      openTimerRef.current = null;
    }
  }, []);

  const closeTooltip = useCallback(() => {
    cancelPendingOpen();
    setTooltipOpen(false);
  }, [cancelPendingOpen]);

  const scheduleTooltip = useCallback(() => {
    cancelPendingOpen();
    openTimerRef.current = window.setTimeout(() => {
      openTimerRef.current = null;
      setTooltipOpen(true);
    }, TOOLTIP_DELAY_MS);
  }, [cancelPendingOpen]);

  useEffect(() => () => cancelPendingOpen(), [cancelPendingOpen]);

  return (
    <>
      <span
        ref={anchorRef}
        className={
          compact
            ? "account-card__credential-warning"
            : "settings-credential-warning"
        }
        role={compact ? "img" : "status"}
        aria-label="凭证失效"
        tabIndex={compact ? undefined : 0}
        onBlur={closeTooltip}
        onFocus={() => {
          cancelPendingOpen();
          setTooltipOpen(true);
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") closeTooltip();
        }}
        onPointerEnter={scheduleTooltip}
        onPointerLeave={closeTooltip}
      >
        <WarningCircle size={compact ? 18 : 14} weight="fill" aria-hidden="true" />
        {compact ? null : <span>凭证失效</span>}
      </span>
      <Tooltip anchorRef={anchorRef} open={tooltipOpen}>
        {HELP_TEXT}
      </Tooltip>
    </>
  );
}
