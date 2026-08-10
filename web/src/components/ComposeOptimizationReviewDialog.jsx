import { useId, useMemo, useRef } from "react";
import { Check, Minus, X } from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { useConfirmDialogFocus } from "./ConfirmDialogPrimitives.jsx";

const MAX_DIFF_CELLS = 1_500_000;

function appendOperation(operations, type, character) {
  const previous = operations.at(-1);
  if (previous?.type === type) {
    previous.text += character;
  } else {
    operations.push({ type, text: character });
  }
}

function fallbackDiff(left, right) {
  let prefix = 0;
  const prefixLimit = Math.min(left.length, right.length);
  while (prefix < prefixLimit && left[prefix] === right[prefix]) prefix += 1;

  let suffix = 0;
  const suffixLimit = Math.min(left.length - prefix, right.length - prefix);
  while (
    suffix < suffixLimit &&
    left[left.length - suffix - 1] === right[right.length - suffix - 1]
  ) {
    suffix += 1;
  }

  const operations = [];
  for (let index = 0; index < prefix; index += 1) {
    appendOperation(operations, "equal", left[index]);
  }
  for (let index = prefix; index < left.length - suffix; index += 1) {
    appendOperation(operations, "delete", left[index]);
  }
  for (let index = prefix; index < right.length - suffix; index += 1) {
    appendOperation(operations, "insert", right[index]);
  }
  for (let index = left.length - suffix; index < left.length; index += 1) {
    appendOperation(operations, "equal", left[index]);
  }
  return operations;
}

function diffCharacters(leftText, rightText) {
  const left = Array.from(leftText || "");
  const right = Array.from(rightText || "");
  const rows = left.length + 1;
  const columns = right.length + 1;

  if (rows * columns > MAX_DIFF_CELLS) return fallbackDiff(left, right);

  const lengths = new Uint32Array(rows * columns);
  for (let leftIndex = left.length - 1; leftIndex >= 0; leftIndex -= 1) {
    const rowOffset = leftIndex * columns;
    const nextRowOffset = (leftIndex + 1) * columns;
    for (let rightIndex = right.length - 1; rightIndex >= 0; rightIndex -= 1) {
      lengths[rowOffset + rightIndex] =
        left[leftIndex] === right[rightIndex]
          ? lengths[nextRowOffset + rightIndex + 1] + 1
          : Math.max(
              lengths[nextRowOffset + rightIndex],
              lengths[rowOffset + rightIndex + 1],
            );
    }
  }

  const operations = [];
  let leftIndex = 0;
  let rightIndex = 0;
  while (leftIndex < left.length && rightIndex < right.length) {
    if (left[leftIndex] === right[rightIndex]) {
      appendOperation(operations, "equal", left[leftIndex]);
      leftIndex += 1;
      rightIndex += 1;
    } else if (
      lengths[(leftIndex + 1) * columns + rightIndex] >=
      lengths[leftIndex * columns + rightIndex + 1]
    ) {
      appendOperation(operations, "delete", left[leftIndex]);
      leftIndex += 1;
    } else {
      appendOperation(operations, "insert", right[rightIndex]);
      rightIndex += 1;
    }
  }
  while (leftIndex < left.length) {
    appendOperation(operations, "delete", left[leftIndex]);
    leftIndex += 1;
  }
  while (rightIndex < right.length) {
    appendOperation(operations, "insert", right[rightIndex]);
    rightIndex += 1;
  }
  return operations;
}

export function buildOptimizationAnnotations(originalText, optimizedText) {
  const left = [];
  const right = [];
  for (const operation of diffCharacters(originalText, optimizedText)) {
    const characters = Array.from(operation.text);
    if (operation.type !== "insert") {
      left.push(
        ...characters.map((character) => ({
          character,
          changed: operation.type === "delete",
        })),
      );
    }
    if (operation.type !== "delete") {
      right.push(
        ...characters.map((character) => ({
          character,
          changed: operation.type === "insert",
        })),
      );
    }
  }
  return { left, right };
}

export function projectOptimizationAnnotations(previous, nextText) {
  const previousText = previous.map(({ character }) => character).join("");
  const next = [];
  let previousIndex = 0;
  for (const operation of diffCharacters(previousText, nextText)) {
    for (const character of Array.from(operation.text)) {
      if (operation.type === "equal") {
        next.push({ character, changed: previous[previousIndex]?.changed || false });
        previousIndex += 1;
      } else if (operation.type === "delete") {
        previousIndex += 1;
      } else {
        next.push({ character, changed: false });
      }
    }
  }
  return next;
}

export function optimizationAnnotationText(annotations) {
  return annotations.map(({ character }) => character).join("");
}

function groupedAnnotations(annotations) {
  const groups = [];
  for (const item of annotations) {
    const previous = groups.at(-1);
    if (previous?.changed === item.changed) {
      previous.text += item.character;
    } else {
      groups.push({ changed: item.changed, text: item.character });
    }
  }
  return groups;
}

function DiffEditor({ annotations, inputRef = null, label, side, onChange }) {
  const highlightRef = useRef(null);
  const text = optimizationAnnotationText(annotations);
  const groups = useMemo(() => groupedAnnotations(annotations), [annotations]);

  return (
    <div className="compose-optimize-diff-editor" data-side={side}>
      <div
        ref={highlightRef}
        className="compose-optimize-diff-editor__highlights"
        aria-hidden="true"
      >
        {groups.map((group, index) => (
          <span key={`${index}-${group.changed}`} data-changed={group.changed || undefined}>
            {group.text}
          </span>
        ))}
      </div>
      <textarea
        ref={inputRef}
        aria-label={label}
        spellCheck="false"
        value={text}
        onChange={(event) =>
          onChange(projectOptimizationAnnotations(annotations, event.target.value))
        }
        onScroll={(event) => {
          if (!highlightRef.current) return;
          highlightRef.current.scrollTop = event.currentTarget.scrollTop;
          highlightRef.current.scrollLeft = event.currentTarget.scrollLeft;
        }}
      />
    </div>
  );
}

export function ComposeOptimizationReviewDialog({
  open,
  leftSubject,
  rightSubject,
  leftAnnotations,
  rightAnnotations,
  returnFocusRef,
  onChangeLeftSubject,
  onChangeRightSubject,
  onChangeLeft,
  onChangeRight,
  onChoose,
  onClose,
  onMinimize,
}) {
  const generatedId = useId().replaceAll(":", "");
  const titleId = `compose-optimize-review-title-${generatedId}`;
  const leftSubjectRef = useRef(null);
  const { dialogRef, onBackdropPointerDown, onDialogKeyDown } =
    useConfirmDialogFocus({
      open,
      initialFocusRef: leftSubjectRef,
      returnFocusRef,
      onCancel: onMinimize,
    });

  if (!open) return null;

  return (
    <div
      className="confirm-layer compose-optimize-review-layer"
      onPointerDown={onBackdropPointerDown}
    >
      <section
        ref={dialogRef}
        className="compose-optimize-review-dialog"
        role="dialog"
        tabIndex={-1}
        aria-modal="true"
        aria-labelledby={titleId}
        onKeyDown={onDialogKeyDown}
      >
        <header className="compose-optimize-review-dialog__header">
          <span aria-hidden="true" />
          <div>
            <h2 id={titleId}>优化结果对比</h2>
            <p>主题与正文按整侧选用；正文差异以红色和绿色标记</p>
          </div>
          <div className="compose-optimize-review-dialog__window-actions">
            <IconButton label="暂时隐藏优化结果" onClick={onMinimize}>
              <Minus size={18} weight="bold" />
            </IconButton>
            <IconButton label="关闭优化结果" onClick={onClose}>
              <X size={18} />
            </IconButton>
          </div>
        </header>

        <div className="compose-optimize-review-dialog__panes">
          <section className="compose-optimize-review-pane" data-side="left">
            <header>
              <div>
                <strong>提交时版本</strong>
                <small>主题与正文可编辑，按整侧选用</small>
              </div>
              <IconButton
                label="整体选用左侧主题与正文"
                onClick={() => onChoose("left")}
              >
                <Check size={18} weight="bold" />
              </IconButton>
            </header>
            <div className="compose-optimize-review-pane__editor">
              <div
                className="compose-optimize-review-field compose-optimize-review-field--subject"
                data-changed={leftSubject !== rightSubject || undefined}
              >
                <label htmlFor={`compose-optimize-left-subject-${generatedId}`}>主题</label>
                <input
                  ref={leftSubjectRef}
                  id={`compose-optimize-left-subject-${generatedId}`}
                  aria-label="编辑左侧主题"
                  value={leftSubject}
                  onChange={(event) => onChangeLeftSubject(event.target.value)}
                />
              </div>
              <div className="compose-optimize-review-field compose-optimize-review-field--body">
                <span>正文</span>
                <DiffEditor
                  annotations={leftAnnotations}
                  label="编辑左侧正文"
                  side="left"
                  onChange={onChangeLeft}
                />
              </div>
            </div>
          </section>

          <section className="compose-optimize-review-pane" data-side="right">
            <header>
              <div>
                <strong>AI 优化后</strong>
                <small>主题与正文可编辑，按整侧选用</small>
              </div>
              <IconButton
                label="整体选用右侧主题与正文"
                onClick={() => onChoose("right")}
              >
                <Check size={18} weight="bold" />
              </IconButton>
            </header>
            <div className="compose-optimize-review-pane__editor">
              <div
                className="compose-optimize-review-field compose-optimize-review-field--subject"
                data-changed={leftSubject !== rightSubject || undefined}
              >
                <label htmlFor={`compose-optimize-right-subject-${generatedId}`}>主题</label>
                <input
                  id={`compose-optimize-right-subject-${generatedId}`}
                  aria-label="编辑右侧主题"
                  value={rightSubject}
                  onChange={(event) => onChangeRightSubject(event.target.value)}
                />
              </div>
              <div className="compose-optimize-review-field compose-optimize-review-field--body">
                <span>正文</span>
                <DiffEditor
                  annotations={rightAnnotations}
                  label="编辑右侧正文"
                  side="right"
                  onChange={onChangeRight}
                />
              </div>
            </div>
          </section>
        </div>
      </section>
    </div>
  );
}
