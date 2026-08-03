import { TooltipTarget } from "./Tooltip.jsx";

export function IconButton({
  label,
  children,
  className = "",
  tone = "default",
  title,
  tooltipOnFocus = true,
  ...props
}) {
  return (
    <TooltipTarget label={title || label} openOnFocus={tooltipOnFocus}>
      <button
        type="button"
        className={`icon-button icon-button--${tone} ${className}`.trim()}
        aria-label={label}
        {...props}
      >
        {children}
      </button>
    </TooltipTarget>
  );
}
