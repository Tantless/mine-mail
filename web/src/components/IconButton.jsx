export function IconButton({
  label,
  children,
  className = "",
  tone = "default",
  ...props
}) {
  return (
    <button
      type="button"
      className={`icon-button icon-button--${tone} ${className}`.trim()}
      aria-label={label}
      title={label}
      {...props}
    >
      {children}
    </button>
  );
}
