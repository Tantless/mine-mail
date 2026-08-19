import { useEffect, useRef, useState } from "react";
import {
  CaretDown,
  Check,
  DotsThree,
  PencilSimple,
  Plus,
  Trash,
  UploadSimple,
  X,
} from "@phosphor-icons/react";
import {
  appearancePalettes,
  builtinAppearanceThemes,
} from "../appearanceThemes.js";
import { themeScheduleIssue } from "../themeSchedule.js";
import {
  ConfirmDialogStatus,
  useConfirmDialogFocus,
} from "./ConfirmDialogPrimitives.jsx";
import { IconButton } from "./IconButton.jsx";
import { ThemedSelect } from "./ThemedSelect.jsx";

const scheduleHourOptions = Array.from({ length: 24 }, (_, hour) => {
  const value = String(hour).padStart(2, "0");
  return { value, label: value };
});

function scheduleMinuteOptions(current) {
  const minutes = Array.from({ length: 12 }, (_, index) => {
    const value = String(index * 5).padStart(2, "0");
    return { value, label: value };
  });
  if (current && !minutes.some((option) => option.value === current)) {
    minutes.push({ value: current, label: current });
  }
  return minutes;
}

function ScheduleTimeRow({ label, value, disabled, onValueChange }) {
  const [hour, minute] = String(value || "06:00").split(":");
  return (
    <div className="appearance-schedule-row">
      <strong className="appearance-schedule-row__label">{label}</strong>
      <span className="appearance-schedule-row__controls">
        <ThemedSelect
          label={`${label}小时`}
          value={hour}
          options={scheduleHourOptions}
          disabled={disabled}
          onValueChange={(nextHour) => onValueChange(`${nextHour}:${minute}`)}
        />
        <span className="appearance-schedule-row__colon" aria-hidden="true">:</span>
        <ThemedSelect
          label={`${label}分钟`}
          value={minute}
          options={scheduleMinuteOptions(minute)}
          disabled={disabled}
          onValueChange={(nextMinute) => onValueChange(`${hour}:${nextMinute}`)}
        />
      </span>
    </div>
  );
}

const supportedTypes = new Set(["image/png", "image/jpeg", "image/webp"]);
const maxSourceBytes = 20 * 1024 * 1024;
function fileDataUrl(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ""));
    reader.onerror = () => reject(new Error("图片读取失败"));
    reader.readAsDataURL(file);
  });
}

async function imageDimensions(file) {
  if (typeof createImageBitmap !== "function") return null;
  try {
    const bitmap = await createImageBitmap(file);
    const dimensions = { width: bitmap.width, height: bitmap.height };
    bitmap.close?.();
    return dimensions;
  } catch {
    return null;
  }
}

function validateFile(file) {
  if (!supportedTypes.has(file?.type)) {
    throw new Error("请选择 PNG、JPEG 或 WebP 图片。");
  }
  if (!file.size || file.size > maxSourceBytes) {
    throw new Error("背景图片不能超过 20 MB。");
  }
}

function PaletteDisc({ palette, selected = false }) {
  return (
    <span className="appearance-palette__disc" aria-hidden="true">
      {palette.swatches.map((color, index) => (
        <span
          key={`${palette.id}-${index}`}
          className="appearance-palette__segment"
          style={{ background: color }}
        />
      ))}
      {selected ? <Check size={16} weight="bold" /> : null}
    </span>
  );
}

function PaletteSwatch({ palette, selected, onSelect, disabled }) {
  return (
    <button
      type="button"
      className="appearance-palette"
      data-selected={selected || undefined}
      aria-label={`${palette.name}${palette.schemeLabel}调色板`}
      aria-pressed={selected}
      title={`${palette.name} · ${palette.schemeLabel}`}
      disabled={disabled}
      onClick={onSelect}
    >
      <PaletteDisc palette={palette} selected={selected} />
    </button>
  );
}

function ThemeCard({
  id,
  name,
  englishName,
  image,
  selected,
  custom = false,
  menuOpen,
  onSelect,
  onMenuToggle,
  onRename,
  onReplace,
  onDelete,
  disabled,
}) {
  return (
    <article
      className="appearance-theme-card"
      data-selected={selected || undefined}
      data-custom={custom || undefined}
    >
      <button
        type="button"
        className={`appearance-theme-card__preview ${custom ? "" : `appearance-theme-card__preview--${id}`}`}
        style={image ? { backgroundImage: `url("${image}")` } : undefined}
        aria-label={`使用${name}主题`}
        aria-pressed={selected}
        onClick={onSelect}
        disabled={disabled}
      >
        {selected ? (
          <span className="appearance-theme-card__selected" aria-hidden="true">
            <Check size={16} weight="bold" />
          </span>
        ) : null}
      </button>
      <div className="appearance-theme-card__meta">
        <span>
          <strong>{name}</strong>
          <small>{englishName}</small>
        </span>
        {custom ? (
          <span className="appearance-theme-card__menu-wrap">
            <IconButton
              className="appearance-theme-card__menu-trigger"
              label={`管理${name}`}
              onClick={onMenuToggle}
              disabled={disabled}
            >
              <DotsThree size={19} weight="bold" />
            </IconButton>
            {menuOpen ? (
              <span className="appearance-theme-card__menu" role="menu">
                <button type="button" role="menuitem" onClick={onRename}>
                  <PencilSimple size={16} />
                  重命名
                </button>
                <button type="button" role="menuitem" onClick={onReplace}>
                  <UploadSimple size={16} />
                  更换图片
                </button>
                <button type="button" role="menuitem" data-tone="danger" onClick={onDelete}>
                  <Trash size={16} />
                  删除
                </button>
              </span>
            ) : null}
          </span>
        ) : null}
      </div>
    </article>
  );
}

export function AppearanceSettings({
  appearance,
  idlePoetryEnabled = true,
  onIdlePoetryEnabledChange,
  themeScheduleEnabled = false,
  themeScheduleDayStart = "06:00",
  themeScheduleDuskStart = "18:00",
  themeScheduleNightStart = "21:00",
  onThemeScheduleChange,
  onSelect,
  onUpdatePreferences,
  onImport,
  onUpdate,
  onDelete,
}) {
  const addInputRef = useRef(null);
  const replaceInputRef = useRef(null);
  const renameInputRef = useRef(null);
  const deleteCancelRef = useRef(null);
  const [busyAction, setBusyAction] = useState(null);
  const [error, setError] = useState(null);
  const [notice, setNotice] = useState(null);
  const [scheduleIssue, setScheduleIssue] = useState(null);
  const [menuId, setMenuId] = useState(null);
  const [replaceId, setReplaceId] = useState(null);
  const [renameId, setRenameId] = useState(null);
  const [renameValue, setRenameValue] = useState("");
  const [deletePreset, setDeletePreset] = useState(null);
  const [focusOpen, setFocusOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const deleteDialogFocus = useConfirmDialogFocus({
    open: Boolean(deletePreset),
    onCancel: () => setDeletePreset(null),
    initialFocusRef: deleteCancelRef,
  });

  const activePreset =
    appearance?.activeTheme?.kind === "custom"
      ? appearance.customPresets.find(
          (preset) => preset.id === appearance.activeTheme.id,
        )
      : null;
  const activePaletteId = appearance?.paletteId || "daylight";
  const activePalette =
    appearancePalettes.find((palette) => palette.id === activePaletteId) ||
    appearancePalettes.find((palette) => palette.id === "daylight");
  const controlsDisabled = Boolean(busyAction);

  useEffect(() => {
    if (!renameId) return;
    renameInputRef.current?.focus();
    renameInputRef.current?.select();
  }, [renameId]);

  useEffect(() => {
    setFocusOpen(false);
  }, [appearance?.activeTheme?.kind, appearance?.activeTheme?.id]);

  const run = async (key, action) => {
    if (busyAction) return;
    setBusyAction(key);
    setError(null);
    setNotice(null);
    try {
      await action();
    } catch (nextError) {
      setError(nextError?.message || "外观设置没有保存，请重试。");
    } finally {
      setBusyAction(null);
    }
  };

  // The schedule is validated before it reaches the save path: an unordered
  // or malformed combination is refused with an inline hint instead of being
  // persisted and then rolled back on a server-side rejection.
  const handleThemeScheduleChange = (patch) => {
    if (patch.themeScheduleEnabled !== undefined) {
      setScheduleIssue(null);
      onThemeScheduleChange?.(patch);
      return;
    }
    const next = {
      dayStart: patch.themeScheduleDayStart ?? themeScheduleDayStart,
      duskStart: patch.themeScheduleDuskStart ?? themeScheduleDuskStart,
      nightStart: patch.themeScheduleNightStart ?? themeScheduleNightStart,
    };
    const issue = themeScheduleIssue(next);
    if (issue) {
      setScheduleIssue(issue);
      return;
    }
    setScheduleIssue(null);
    onThemeScheduleChange?.(patch);
  };

  const importFile = async (file, presetId = null) => {
    validateFile(file);
    const [imageDataUrl, dimensions] = await Promise.all([
      fileDataUrl(file),
      imageDimensions(file),
    ]);
    if (presetId) {
      await onUpdate({ id: presetId, imageDataUrl });
    } else {
      await onImport({ imageDataUrl });
    }
    if (dimensions && (dimensions.width < 1_600 || dimensions.height < 900)) {
      setNotice("图片已保存，但分辨率低于建议的 1600 × 900，放大窗口时可能不够清晰。");
    }
  };

  const beginRename = (preset) => {
    setMenuId(null);
    setRenameId(preset.id);
    setRenameValue(preset.name);
  };

  const saveRename = () => {
    const value = renameValue.trim();
    if (!renameId || !value) {
      setRenameId(null);
      return;
    }
    void run(`rename:${renameId}`, async () => {
      await onUpdate({ id: renameId, name: value });
      setRenameId(null);
    });
  };

  const updateFocalPoint = (event) => {
    if (!activePreset || busyAction) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const focalX = Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width));
    const focalY = Math.max(0, Math.min(1, (event.clientY - rect.top) / rect.height));
    void run(`focal:${activePreset.id}`, () =>
      onUpdate({ id: activePreset.id, focalX, focalY }),
    );
  };

  return (
    <section className="appearance-settings" aria-labelledby="settings-appearance-title">
      <header className="settings-page__heading">
        <span>
          <p className="eyebrow">APPEARANCE</p>
          <h3 id="settings-appearance-title">外观</h3>
          <p>使用纯色极简界面，或保留主题背景的氛围与层次。</p>
        </span>
      </header>

      <section className="appearance-section appearance-preference-bar" aria-label="极简模式">
        <label className="settings-preference-row settings-preference-row--toggle">
          <span>
            <strong>极简模式</strong>
            <small>隐藏主题图片，以当前调色盘呈现接近纯色的区块。</small>
          </span>
          <input
            type="checkbox"
            aria-label="启用极简模式"
            checked={appearance?.minimalModeEnabled !== false}
            disabled={controlsDisabled}
            onChange={(event) =>
              void run("minimal-mode", () =>
                onUpdatePreferences({
                  minimalModeEnabled: event.target.checked,
                }),
              )
            }
          />
        </label>
      </section>

      <section
        className="appearance-section appearance-config"
        aria-labelledby="appearance-palette-title"
      >
        <header className="appearance-section__heading">
          <span>
            <strong id="appearance-palette-title">调色盘</strong>
            <small>极简模式完全由当前色盘配色；四个内置主题的原色也可单独选用。</small>
          </span>
        </header>
        <div className="appearance-config__rows">
          <button
            type="button"
            className="appearance-config__row appearance-config__disclosure"
            aria-expanded={paletteOpen}
            aria-controls="appearance-palette-panel"
            disabled={controlsDisabled}
            onClick={() => setPaletteOpen((current) => !current)}
          >
            <span>
              <strong>界面配色</strong>
              <small>
                {activePalette
                  ? `${activePalette.name} · ${activePalette.schemeLabel}`
                  : "日间原色 · 明亮"}
              </small>
            </span>
            <span className="appearance-config__palette-value" aria-hidden="true">
              <PaletteDisc palette={activePalette} />
              <CaretDown size={18} weight="bold" />
            </span>
          </button>
          {paletteOpen ? (
            <div id="appearance-palette-panel" className="appearance-config__panel appearance-config__panel--palette">
              <p>
                色盘统一控制背景、面板、文字、边框、交互色和状态色。关闭极简模式后，选择内置主题会恢复它的原色，你仍可在这里改用其他色盘。
              </p>
              {[
                { scheme: "light", label: "明亮界面" },
                { scheme: "dark", label: "深色界面" },
              ].map((group) => (
                <section
                  key={group.scheme}
                  className="appearance-palette-group"
                  aria-label={group.label}
                >
                  <strong>{group.label}</strong>
                  <div className="appearance-palette-list">
                    {appearancePalettes
                      .filter((palette) => palette.scheme === group.scheme)
                      .map((palette) => (
                        <PaletteSwatch
                          key={palette.id}
                          palette={palette}
                          selected={activePaletteId === palette.id}
                          disabled={controlsDisabled}
                          onSelect={() =>
                            void run(`palette:${palette.id}`, () =>
                              onUpdatePreferences({ paletteId: palette.id }),
                            )
                          }
                        />
                      ))}
                  </div>
                </section>
              ))}
            </div>
          ) : null}
        </div>
      </section>

      <section className="appearance-section" aria-labelledby="appearance-themes-title">
        <header className="appearance-section__heading">
          <span>
            <strong id="appearance-themes-title">主题背景</strong>
            <small>
              {appearance?.minimalModeEnabled !== false
                ? "极简模式下保留选择与设置，但不会显示背景图片。"
                : "内置主题默认使用各自原色；也可以把自己的图片保存为预设。"}
            </small>
          </span>
        </header>
        <div className="appearance-theme-grid">
          {builtinAppearanceThemes.map((theme) => (
            <ThemeCard
              key={theme.id}
              {...theme}
              selected={
                appearance?.activeTheme?.kind === "builtin" &&
                appearance.activeTheme.id === theme.id
              }
              disabled={Boolean(busyAction)}
              onSelect={() =>
                void run(`select:${theme.id}`, () =>
                  onSelect({ kind: "builtin", id: theme.id }),
                )
              }
            />
          ))}
          {(appearance?.customPresets || []).map((preset) => (
            <div className="appearance-theme-card-wrap" key={preset.id}>
              <ThemeCard
                id={preset.id}
                name={preset.name}
                englishName="CUSTOM"
                image={preset.thumbnailDataUrl}
                custom
                selected={appearance.activeTheme.id === preset.id}
                menuOpen={menuId === preset.id}
                disabled={Boolean(busyAction)}
                onSelect={() =>
                  void run(`select:${preset.id}`, () =>
                    onSelect({ kind: "custom", id: preset.id }),
                  )
                }
                onMenuToggle={() =>
                  setMenuId((current) => (current === preset.id ? null : preset.id))
                }
                onRename={() => beginRename(preset)}
                onReplace={() => {
                  setMenuId(null);
                  setReplaceId(preset.id);
                  replaceInputRef.current?.click();
                }}
                onDelete={() => {
                  setMenuId(null);
                  setDeletePreset(preset);
                }}
              />
              {renameId === preset.id ? (
                <form
                  className="appearance-theme-card__rename"
                  onSubmit={(event) => {
                    event.preventDefault();
                    saveRename();
                  }}
                >
                  <input
                    ref={renameInputRef}
                    value={renameValue}
                    maxLength={40}
                    aria-label="自定义主题名称"
                    onChange={(event) => setRenameValue(event.target.value)}
                    onBlur={saveRename}
                  />
                  <IconButton label="取消重命名" onMouseDown={(event) => event.preventDefault()} onClick={() => setRenameId(null)}>
                    <X size={15} />
                  </IconButton>
                </form>
              ) : null}
            </div>
          ))}
          <button
            type="button"
            className="appearance-add-card"
            aria-label="添加自定义主题"
            onClick={() => addInputRef.current?.click()}
            disabled={Boolean(busyAction)}
          >
            <span><Plus size={23} weight="bold" /></span>
            <strong>添加自定义主题</strong>
            <small>PNG、JPEG 或 WebP，最大 20 MB</small>
          </button>
        </div>
        <input
          ref={addInputRef}
          className="sr-only"
          type="file"
          tabIndex={-1}
          aria-label="选择自定义背景图片"
          accept="image/png,image/jpeg,image/webp"
          onChange={(event) => {
            const file = event.target.files?.[0];
            event.target.value = "";
            if (file) void run("import", () => importFile(file));
          }}
        />
        <input
          ref={replaceInputRef}
          className="sr-only"
          type="file"
          tabIndex={-1}
          aria-label="更换自定义背景图片"
          accept="image/png,image/jpeg,image/webp"
          onChange={(event) => {
            const file = event.target.files?.[0];
            event.target.value = "";
            const id = replaceId;
            setReplaceId(null);
            if (file && id) void run(`replace:${id}`, () => importFile(file, id));
          }}
        />
        {activePreset ? (
          <div className="appearance-config__rows appearance-theme-focus">
          <button
            type="button"
            className="appearance-config__row appearance-config__disclosure"
            aria-expanded={focusOpen}
            aria-controls="appearance-focus-panel"
            disabled={controlsDisabled}
            onClick={() => setFocusOpen((current) => !current)}
          >
            <span>
              <strong>设置焦点</strong>
              <small>决定背景裁切时优先保留的位置</small>
            </span>
            <CaretDown size={18} weight="bold" aria-hidden="true" />
          </button>
          {activePreset && focusOpen ? (
            <div id="appearance-focus-panel" className="appearance-config__panel">
              <button
                type="button"
                className="appearance-focal-preview"
                style={{
                  backgroundImage: `url("${activePreset.thumbnailDataUrl}")`,
                  backgroundPosition: `${activePreset.focalX * 100}% ${activePreset.focalY * 100}%`,
                }}
                onPointerDown={updateFocalPoint}
                aria-label="背景焦点。点击图片设置希望保持可见的位置"
                disabled={Boolean(busyAction)}
              >
                <span
                  className="appearance-focal-preview__marker"
                  style={{
                    left: `${activePreset.focalX * 100}%`,
                    top: `${activePreset.focalY * 100}%`,
                  }}
                  aria-hidden="true"
                />
              </button>
              <p>点击预览中最重要的位置，窗口裁切时会优先保留这里。</p>
            </div>
          ) : null}
          </div>
        ) : null}
      </section>

      <section className="appearance-section" aria-labelledby="appearance-schedule-title">
        <header className="appearance-section__heading">
          <span>
            <strong id="appearance-schedule-title">定时切换主题背景</strong>
            <small>随本机时间切换背景；极简模式开启时选择会保留但不显示。</small>
          </span>
          <input
            className="appearance-config__toggle"
            type="checkbox"
            aria-label="定时切换主题背景"
            checked={themeScheduleEnabled}
            disabled={Boolean(busyAction)}
            onChange={(event) =>
              handleThemeScheduleChange({ themeScheduleEnabled: event.target.checked })
            }
          />
        </header>
        {themeScheduleEnabled ? (
          <div className="appearance-schedule-list">
            <ScheduleTimeRow
              label="日间开始"
              value={themeScheduleDayStart}
              disabled={Boolean(busyAction)}
              onValueChange={(themeScheduleDayStart) =>
                handleThemeScheduleChange({ themeScheduleDayStart })
              }
            />
            <ScheduleTimeRow
              label="黄昏开始"
              value={themeScheduleDuskStart}
              disabled={Boolean(busyAction)}
              onValueChange={(themeScheduleDuskStart) =>
                handleThemeScheduleChange({ themeScheduleDuskStart })
              }
            />
            <ScheduleTimeRow
              label="夜间开始"
              value={themeScheduleNightStart}
              disabled={Boolean(busyAction)}
              onValueChange={(themeScheduleNightStart) =>
                handleThemeScheduleChange({ themeScheduleNightStart })
              }
            />
          </div>
        ) : null}
        {scheduleIssue ? (
          <p className="appearance-schedule__error" role="alert" aria-live="assertive">
            {scheduleIssue}时间需按日间、黄昏、夜间依次排列。
          </p>
        ) : null}
        {themeScheduleEnabled ? (
          <p className="appearance-schedule__hint">
            时间按日间、黄昏、夜间依次排列，夜间持续到次日日间开始。
          </p>
        ) : null}
      </section>

      <section className="appearance-section appearance-preference-bar" aria-label="主页展示">
        <label className="settings-preference-row settings-preference-row--toggle">
          <span>
            <strong>主页诗歌</strong>
            <small>未打开邮件时，在阅读区展示随机诗句。</small>
          </span>
          <input
            type="checkbox"
            aria-label="展示主页诗歌"
            checked={idlePoetryEnabled}
            disabled={Boolean(busyAction)}
            onChange={(event) =>
              onIdlePoetryEnabledChange?.(event.target.checked)
            }
          />
        </label>
      </section>

      {error || notice || busyAction ? (
        <p
          className="appearance-settings__status"
          data-tone={error ? "danger" : undefined}
          role={error ? "alert" : "status"}
          aria-live={error ? "assertive" : "polite"}
        >
          {error || notice || "正在保存外观设置…"}
        </p>
      ) : null}

      {deletePreset ? (
        <div className="confirm-layer" onPointerDown={deleteDialogFocus.onBackdropPointerDown}>
          <section
            ref={deleteDialogFocus.dialogRef}
            className="confirm-dialog"
            role="dialog"
            tabIndex={-1}
            aria-modal="true"
            aria-labelledby="appearance-delete-title"
            aria-describedby="appearance-delete-description"
            onKeyDown={deleteDialogFocus.onDialogKeyDown}
          >
            <header>
              <span className="confirm-dialog__icon" aria-hidden="true"><Trash size={22} weight="duotone" /></span>
              <IconButton label="取消删除主题" onClick={() => setDeletePreset(null)}><X size={18} /></IconButton>
            </header>
            <h2 id="appearance-delete-title">删除“{deletePreset.name}”？</h2>
            <p id="appearance-delete-description">这个预设和 Mine Mail 管理的背景副本会一并删除，原始图片不会受到影响。</p>
            <footer>
              <button ref={deleteCancelRef} type="button" className="secondary-button" onClick={() => setDeletePreset(null)}>取消</button>
              <button
                type="button"
                className="danger-button"
                onClick={() => {
                  const id = deletePreset.id;
                  setDeletePreset(null);
                  void run(`delete:${id}`, () => onDelete(id));
                }}
              >
                删除主题
              </button>
            </footer>
            <ConfirmDialogStatus>{busyAction?.startsWith("delete:") ? "正在删除主题…" : null}</ConfirmDialogStatus>
          </section>
        </div>
      ) : null}
    </section>
  );
}
