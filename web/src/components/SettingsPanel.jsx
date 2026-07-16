import { useEffect, useState } from "react";
import { Check, GearSix, Question, X } from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { AccountSetupForm } from "./AccountSetup.jsx";
import { ProfileAvatar } from "./ProfileAvatar.jsx";

const intervalOptions = [1, 3, 5];
const remoteImageOptions = [
  { id: "automatic", label: "自动加载" },
  { id: "ask", label: "每次询问" },
  { id: "blocked", label: "始终阻止" },
];
const remoteImageRisk =
  "自动加载会连接发件人的图片服务器，可能暴露邮件打开时间、IP 地址和设备信息，并让追踪像素确认邮箱处于活跃状态。";

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
  accountAvatar,
  onSetAccountAvatar,
  onRemoveAccountAvatar,
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
            {accountStatus?.configured && accountStatus.email ? (
              <div className="account-avatar-setting">
                <ProfileAvatar
                  className="account-avatar-setting__preview"
                  email={accountStatus.email}
                  label={accountStatus.email}
                  customSrc={accountAvatar}
                />
                <span className="account-avatar-setting__copy">
                  <strong>Mine Mail 头像</strong>
                  <small>仅保存在这台设备上，不会修改邮箱服务商的头像。</small>
                </span>
                <label className="secondary-button account-avatar-setting__choose">
                  {accountAvatar ? "更换" : "选择图片"}
                  <input
                    type="file"
                    accept="image/png,image/jpeg,image/webp"
                    aria-label="选择 Mine Mail 账户头像"
                    onChange={(event) => {
                      const file = event.target.files?.[0];
                      if (file) void onSetAccountAvatar(file);
                      event.target.value = "";
                    }}
                  />
                </label>
                {accountAvatar ? (
                  <button
                    type="button"
                    className="settings-text-button"
                    onClick={onRemoveAccountAvatar}
                  >
                    移除
                  </button>
                ) : null}
              </div>
            ) : null}
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

          <div className="settings-section">
            <div className="settings-section__heading">
              <div className="settings-section__title-row">
                <strong>远程图片</strong>
                <span className="settings-help">
                  <button
                    type="button"
                    className="settings-help__button"
                    aria-label="了解自动加载远程图片的隐私风险"
                    aria-describedby="remote-image-risk"
                  >
                    <Question size={13} weight="bold" />
                  </button>
                  <span
                    id="remote-image-risk"
                    className="settings-help__tooltip"
                    role="tooltip"
                  >
                    {remoteImageRisk}
                  </span>
                </span>
              </div>
              <span>控制 HTML 邮件是否连接外部图片服务器。</span>
            </div>
            <div className="settings-options" role="radiogroup" aria-label="远程图片加载方式">
              {remoteImageOptions.map((option) => (
                <button
                  key={option.id}
                  type="button"
                  role="radio"
                  aria-checked={value.remoteImageMode === option.id}
                  data-selected={value.remoteImageMode === option.id}
                  onClick={() =>
                    setValue((current) => ({
                      ...current,
                      remoteImageMode: option.id,
                    }))
                  }
                >
                  {option.label}
                  {value.remoteImageMode === option.id ? <Check size={15} weight="bold" /> : null}
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
