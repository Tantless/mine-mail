import { useEffect, useId, useMemo, useRef, useState } from "react";
import { CaretDown, Check } from "@phosphor-icons/react";

function optionIndex(options, value) {
  return Math.max(
    0,
    options.findIndex((option) => String(option.value) === String(value)),
  );
}

export function ThemedSelect({
  id,
  label,
  value,
  options,
  onValueChange,
  disabled = false,
  className = "",
}) {
  const generatedId = useId();
  const listboxId = `${id || generatedId}-listbox`;
  const rootRef = useRef(null);
  const triggerRef = useRef(null);
  const optionRefs = useRef([]);
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(() => optionIndex(options, value));
  const selected = useMemo(
    () => options.find((option) => String(option.value) === String(value)) || options[0],
    [options, value],
  );

  useEffect(() => {
    setActiveIndex(optionIndex(options, value));
  }, [options, value]);

  useEffect(() => {
    if (!open) return undefined;
    const handlePointerDown = (event) => {
      if (!rootRef.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", handlePointerDown);
    return () => document.removeEventListener("pointerdown", handlePointerDown);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    window.requestAnimationFrame(() => optionRefs.current[activeIndex]?.focus());
  }, [activeIndex, open]);

  const closeAndFocus = () => {
    setOpen(false);
    window.requestAnimationFrame(() => triggerRef.current?.focus());
  };

  const choose = (option) => {
    if (option.disabled) return;
    onValueChange(option.value);
    closeAndFocus();
  };

  const move = (offset) => {
    if (!options.length) return;
    let next = activeIndex;
    do {
      next = (next + offset + options.length) % options.length;
    } while (options[next]?.disabled && next !== activeIndex);
    setActiveIndex(next);
  };

  const handleTriggerKeyDown = (event) => {
    if (disabled) return;
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      const next =
        event.key === "Home"
          ? 0
          : event.key === "End"
            ? options.length - 1
            : optionIndex(options, value);
      setActiveIndex(next);
      setOpen(true);
    }
  };

  const handleListKeyDown = (event) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      move(event.key === "ArrowDown" ? 1 : -1);
      return;
    }
    if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      setActiveIndex(event.key === "Home" ? 0 : options.length - 1);
      return;
    }
    if (event.key === "Escape" || event.key === "Tab") {
      if (event.key === "Escape") event.preventDefault();
      setOpen(false);
      if (event.key === "Escape") triggerRef.current?.focus();
    }
  };

  return (
    <span
      ref={rootRef}
      className={`themed-select ${className}`.trim()}
      data-open={open}
      data-disabled={disabled}
    >
      <button
        ref={triggerRef}
        id={id}
        type="button"
        role="combobox"
        className="themed-select__trigger"
        aria-label={label}
        aria-controls={listboxId}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-activedescendant={open ? `${listboxId}-option-${activeIndex}` : undefined}
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={handleTriggerKeyDown}
      >
        <span>{selected?.label || "请选择"}</span>
        <CaretDown className="themed-select__caret" size={15} weight="bold" aria-hidden="true" />
      </button>

      {open ? (
        <span
          id={listboxId}
          className="themed-select__menu"
          role="listbox"
          aria-label={label}
          onKeyDown={handleListKeyDown}
        >
          {options.map((option, index) => {
            const isSelected = String(option.value) === String(value);
            return (
              <button
                key={String(option.value)}
                ref={(element) => {
                  optionRefs.current[index] = element;
                }}
                id={`${listboxId}-option-${index}`}
                type="button"
                role="option"
                className="themed-select__option"
                aria-selected={isSelected}
                data-active={activeIndex === index}
                disabled={option.disabled}
                onFocus={() => setActiveIndex(index)}
                onClick={() => choose(option)}
              >
                <span>{option.label}</span>
                <Check
                  className="themed-select__check"
                  size={15}
                  weight="bold"
                  aria-hidden="true"
                />
              </button>
            );
          })}
        </span>
      ) : null}
    </span>
  );
}
