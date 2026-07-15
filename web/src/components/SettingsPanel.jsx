import { useEffect, useState } from "react";
import { Check, GearSix, X } from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { AccountSetupForm } from "./AccountSetup.jsx";

const intervalOptions = [1, 3, 5];

export function SettingsPanel({
  settings,
  saveStatus,
  onClose,
  onSave,
  accountPresets,
  accountStatus,
  accountSubmitStatus,
  accountError,
  onConfigureAccount,
}) {
  const [value, setValue] = useState(settings);

  useEffect(() => {
    setValue(settings);
  }, [settings]);

  return (
    <div className="settings-layer">
      <section
        className="settings-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
      >
        <header className="settings-header">
          <span className="settings-header__icon">
            <GearSix size={21} weight="duotone" />
          </span>
          <div>
            <p className="eyebrow">DESKTOP</p>
            <h2 id="settings-title">桌面设置</h2>
          </div>
          <IconButton label="关闭设置" onClick={onClose} disabled={saveStatus === "saving"}>
            <X size={18} />
          </IconButton>
        </header>

        <div className="settings-scroll">
          <div className="settings-section">
            <div className="settings-section__heading">
              <strong>邮箱账户</strong>
              <span>
                {accountStatus?.configured
                  ? `当前账户：${accountStatus.email || "已配置"}`
                  : "连接邮箱后才能同步和发送邮件。"}
              </span>
            </div>
            <AccountSetupForm
              presets={accountPresets}
              status={accountStatus}
              submitStatus={accountSubmitStatus}
              error={accountError}
              onSubmit={onConfigureAccount}
            />
          </div>

          <div className="settings-section">
          <div className="settings-section__heading">
            <strong>自动同步</strong>
            <span>应用运行时按此频率检查新邮件。</span>
          </div>
          <div className="settings-options" role="radiogroup" aria-label="自动同步间隔">
            {intervalOptions.map((minutes) => (
              <button
                key={minutes}
                type="button"
                role="radio"
                aria-checked={value.pollingIntervalMinutes === minutes}
                data-selected={value.pollingIntervalMinutes === minutes}
                onClick={() =>
                  setValue((current) => ({
                    ...current,
                    pollingIntervalMinutes: minutes,
                  }))
                }
              >
                {minutes} 分钟
                {value.pollingIntervalMinutes === minutes ? <Check size={15} weight="bold" /> : null}
              </button>
            ))}
          </div>
          </div>

          <label className="settings-toggle">
          <span>
            <strong>开机启动</strong>
            <small>默认关闭；只在你主动开启后随系统启动。</small>
          </span>
          <input
            type="checkbox"
            checked={value.autostartEnabled}
            onChange={(event) =>
              setValue((current) => ({
                ...current,
                autostartEnabled: event.target.checked,
              }))
            }
          />
          </label>

          {saveStatus === "error" ? (
            <p className="settings-error" role="alert">设置没有保存，请重试。</p>
          ) : null}
        </div>

        <footer className="settings-footer">
          <button type="button" className="secondary-button" onClick={onClose} disabled={saveStatus === "saving"}>
            取消
          </button>
          <button
            type="button"
            className="send-button"
            onClick={() => onSave(value)}
            disabled={saveStatus === "saving"}
          >
            {saveStatus === "saving" ? "正在保存…" : "保存设置"}
          </button>
        </footer>
      </section>
    </div>
  );
}
