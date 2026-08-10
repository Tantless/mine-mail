import { useEffect, useId, useRef, useState } from "react";
import {
  CaretDown,
  Check,
  SpinnerGap,
  Translate,
} from "@phosphor-icons/react";
import { useBoundedDropdown } from "../hooks/useBoundedDropdown.js";
import { TooltipTarget } from "./Tooltip.jsx";

function selectedIndex(options, value) {
  return Math.max(
    0,
    options.findIndex((option) => String(option.value) === String(value)),
  );
}

export function ReaderTranslationLanguageSelect({
  value,
  options,
  onValueChange,
  disabled = false,
  busy = false,
  className = "",
}) {
  const listboxId = `${useId()}-reader-translation-languages`;
  const rootRef = useRef(null);
  const triggerRef = useRef(null);
  const optionRefs = useRef([]);
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(() =>
    selectedIndex(options, value),
  );
  const menuLayout = useBoundedDropdown({
    open,
    anchorRef: rootRef,
    preferredMaxHeight: 154,
  });
  const selected =
    options.find((option) => String(option.value) === String(value))
    || options[0];

  useEffect(() => {
    setActiveIndex(selectedIndex(options, value));
  }, [options, value]);

  useEffect(() => {
    if (!open) return undefined;
    const closeFromOutside = (event) => {
      if (!rootRef.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", closeFromOutside);
    return () => document.removeEventListener("pointerdown", closeFromOutside);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    window.requestAnimationFrame(() => {
      optionRefs.current[activeIndex]?.scrollIntoView?.({ block: "nearest" });
    });
  }, [activeIndex, open]);

  const closeAndFocus = () => {
    setOpen(false);
    window.requestAnimationFrame(() => triggerRef.current?.focus());
  };

  const choose = (option) => {
    if (!option || option.disabled) return;
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
    if (event.key === "Escape" && open) {
      event.preventDefault();
      setOpen(false);
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) {
        setActiveIndex(selectedIndex(options, value));
        setOpen(true);
      } else {
        move(event.key === "ArrowDown" ? 1 : -1);
      }
      return;
    }
    if (open && (event.key === "Enter" || event.key === " ")) {
      event.preventDefault();
      choose(options[activeIndex]);
      return;
    }
    if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      setActiveIndex(event.key === "Home" ? 0 : options.length - 1);
      setOpen(true);
    }
  };

  return (
    <div
      ref={rootRef}
      className={["reader-translation-language-select", className]
        .filter(Boolean)
        .join(" ")}
      data-open={open || undefined}
      data-busy={busy || undefined}
    >
      <button
        ref={triggerRef}
        type="button"
        role="combobox"
        className="reader-translation-language-trigger"
        aria-label="AI 翻译语言"
        aria-controls={listboxId}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-activedescendant={
          open ? `${listboxId}-option-${activeIndex}` : undefined
        }
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={handleTriggerKeyDown}
      >
        <span>{selected?.label || "选择语言"}</span>
        {busy ? (
          <SpinnerGap className="spin" size={13} aria-hidden="true" />
        ) : (
          <CaretDown size={13} weight="bold" aria-hidden="true" />
        )}
      </button>

      {open ? (
        <div
          id={listboxId}
          className="reader-translation-language-menu vertical-scroll-surface"
          role="listbox"
          aria-label="AI 翻译语言"
          style={{ maxHeight: `${menuLayout.maxHeight}px` }}
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
                className="reader-translation-language-option"
                aria-selected={isSelected}
                data-active={activeIndex === index || undefined}
                disabled={option.disabled}
                onPointerMove={() => setActiveIndex(index)}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => choose(option)}
              >
                <span>{option.label}</span>
                {isSelected ? <Check size={14} weight="bold" aria-hidden="true" /> : null}
              </button>
            );
          })}
        </div>
      ) : null}

      {busy ? <span className="sr-only" role="status">AI 翻译中</span> : null}
    </div>
  );
}

export function ReaderTranslationControl({
  value,
  options,
  onRun,
  onValueChange,
  runDisabled = false,
  selectDisabled = false,
  translating = false,
  retry = false,
}) {
  const selected =
    options.find((option) => String(option.value) === String(value))
    || options[0];

  return (
    <div
      className="reader-translation-control"
      data-translating={translating || undefined}
    >
      <TooltipTarget label={`${retry ? "重试翻译" : "翻译"}为${selected?.label || "所选语言"}`}>
        <button
          type="button"
          className="reader-translation-run"
          aria-label="AI 翻译"
          aria-busy={translating}
          disabled={runDisabled}
          onClick={onRun}
        >
          {translating ? (
            <SpinnerGap className="spin" size={16} aria-hidden="true" />
          ) : (
            <Translate size={16} aria-hidden="true" />
          )}
        </button>
      </TooltipTarget>
      <ReaderTranslationLanguageSelect
        value={value}
        options={options}
        disabled={selectDisabled}
        busy={translating}
        onValueChange={onValueChange}
      />
    </div>
  );
}
