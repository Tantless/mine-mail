import { useEffect, useRef, useState } from "react";
import {
  CheckCircle,
  CaretDown,
  GearSix,
  Info,
  Plus,
  Question,
  SlidersHorizontal,
  UserCircle,
  X,
} from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { AccountSetupForm } from "./AccountSetup.jsx";
import { ProfileAvatar } from "./ProfileAvatar.jsx";

const remoteImageOptions = [
  { id: "automatic", label: "自动加载" },
  { id: "ask", label: "每次询问" },
  { id: "blocked", label: "始终阻止" },
];

const notificationSoundOptions = [
  { id: "mail", label: "邮件提示音" },
  { id: "default", label: "系统默认" },
  { id: "im", label: "轻柔提示" },
  { id: "reminder", label: "提醒提示" },
];

const menuItems = [
  {
    id: "account",
    label: "账户",
    description: "连接与管理邮箱",
    icon: UserCircle,
  },
  {
    id: "features",
    label: "功能设定",
    description: "同步、通知、图片与启动",
    icon: SlidersHorizontal,
  },
  {
    id: "version",
    label: "版本",
    description: "版本与更新",
    icon: Info,
  },
];

const providerNames = {
  "163": "163 邮箱",
  gmail: "Gmail",
  outlook: "Outlook",
  custom: "自定义邮箱",
};

const remoteImageRisk =
  "自动加载会连接发件人的图片服务器，可能暴露邮件打开时间、IP 地址和设备信息，并让追踪像素确认邮箱处于活跃状态。";

function SettingsSelect({ id, label, value, onChange, disabled = false, children }) {
  return (
    <span className="settings-select-wrap">
      <select
        id={id}
        aria-label={label}
        value={value}
        onChange={onChange}
        disabled={disabled}
      >
        {children}
      </select>
      <CaretDown size={15} weight="bold" aria-hidden="true" />
    </span>
  );
}

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
  onConnectGoogle,
  onSwitchAccount,
  onRemoveAccount,
  accountAvatar,
  onSetAccountAvatar,
  onRemoveAccountAvatar,
  focusTarget,
}) {
  const [value, setValue] = useState(settings);
  const [activeSection, setActiveSection] = useState("account");
  const accountFormRef = useRef(null);

  useEffect(() => {
    setValue(settings);
  }, [settings]);

  useEffect(() => {
    if (focusTarget !== "account-form") return undefined;
    setActiveSection("account");
    const scheduleFrame = window.requestAnimationFrame || window.setTimeout;
    const cancelFrame = window.cancelAnimationFrame || window.clearTimeout;
    const frame = scheduleFrame(() => {
      accountFormRef.current?.scrollIntoView?.({ block: "start", behavior: "smooth" });
    });
    return () => cancelFrame(frame);
  }, [focusTarget]);

  return (
    <div className="settings-layer">
      <section
        className="settings-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
      >
        <IconButton
          className="settings-close"
          label="关闭设置"
          onClick={onClose}
          disabled={saveStatus === "saving"}
        >
          <X size={19} />
        </IconButton>

        <aside className="settings-sidebar">
          <div className="settings-brand">
            <span className="settings-brand__icon">
              <GearSix size={24} weight="duotone" />
            </span>
            <span>
              <small>MINE MAIL</small>
              <h2 id="settings-title">设置</h2>
            </span>
          </div>

          <nav className="settings-nav" aria-label="设置菜单">
            {menuItems.map((item) => {
              const MenuIcon = item.icon;
              const selected = activeSection === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  aria-current={selected ? "page" : undefined}
                  data-selected={selected}
                  onClick={() => setActiveSection(item.id)}
                >
                  <span className="settings-nav__icon">
                    <MenuIcon size={20} weight={selected ? "fill" : "regular"} />
                  </span>
                  <span className="settings-nav__copy">
                    <strong>{item.label}</strong>
                    <small>{item.description}</small>
                  </span>
                </button>
              );
            })}
          </nav>

          <p className="settings-sidebar__note">偏好设置仅保存在这台设备上。</p>
        </aside>

        <div className="settings-content">
          <div className="settings-scroll vertical-scroll-surface">
            {activeSection === "account" ? (
              <section className="settings-page" aria-labelledby="settings-account-title">
                <header className="settings-page__heading">
                  <p className="eyebrow">ACCOUNT</p>
                  <h3 id="settings-account-title">账户</h3>
                  <p>查看已连接的邮箱，或建立新的账户连接。</p>
                </header>

                <div className="settings-subsection">
                  <div className="settings-subsection__heading">
                    <span>
                      <strong>已连接账户</strong>
                      <small>Mine Mail 当前用于同步与发送邮件的账户。</small>
                    </span>
                    {accountStatus?.configured ? (
                      <span className="settings-status-chip">
                        <CheckCircle size={15} weight="fill" />
                        {accountStatus.accountCount || accountStatus.accounts?.length || 1}/
                        {accountStatus.maxAccounts || 3}
                      </span>
                    ) : null}
                  </div>

                  {accountStatus?.configured && accountStatus.accounts?.length ? (
                    <div className="settings-account-list">
                      {accountStatus.accounts.map((connectedAccount) => {
                        const active =
                          connectedAccount.accountId === accountStatus.activeAccountId;
                        return (
                          <div
                            className="settings-account-card"
                            data-active={active}
                            key={connectedAccount.accountId}
                          >
                            <ProfileAvatar
                              className="settings-account-card__avatar"
                              email={connectedAccount.email}
                              label={connectedAccount.email}
                              customSrc={active ? accountAvatar : null}
                            />
                            <span className="settings-account-card__copy">
                              <strong>{connectedAccount.email}</strong>
                              <small>
                                {providerNames[connectedAccount.provider] || "邮箱账户"}
                                {active ? " · 当前账户" : ""}
                              </small>
                            </span>
                            <span className="settings-account-card__actions">
                              {active ? (
                                <label className="secondary-button settings-account-card__avatar-action">
                                  {accountAvatar ? "更换头像" : "设置头像"}
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
                              ) : (
                                <button
                                  type="button"
                                  className="secondary-button"
                                  onClick={() => onSwitchAccount(connectedAccount.accountId)}
                                >
                                  切换
                                </button>
                              )}
                              <button
                                type="button"
                                className="settings-text-button settings-text-button--danger"
                                onClick={() => onRemoveAccount(connectedAccount)}
                              >
                                移除账户
                              </button>
                              {active && accountAvatar ? (
                                <button
                                  type="button"
                                  className="settings-text-button"
                                  onClick={onRemoveAccountAvatar}
                                >
                                  移除头像
                                </button>
                              ) : null}
                            </span>
                          </div>
                        );
                      })}
                    </div>
                  ) : (
                    <div className="settings-account-empty">
                      <UserCircle size={29} weight="duotone" />
                      <span>
                        <strong>尚未连接邮箱</strong>
                        <small>完成下方配置后即可开始同步邮件。</small>
                      </span>
                    </div>
                  )}
                </div>

                <div
                  ref={accountFormRef}
                  className="settings-subsection settings-subsection--account-form"
                >
                  <div className="settings-subsection__heading">
                    <span>
                      <strong className="settings-heading-with-icon">
                        <Plus size={16} weight="bold" />
                        连接新账户
                      </strong>
                      <small>选择服务商，并使用邮箱服务商生成的客户端授权信息。</small>
                    </span>
                  </div>
                  {accountStatus?.canAddAccount === false ? (
                    <div className="settings-account-empty">
                      <Info size={25} weight="duotone" />
                      <span>
                        <strong>已达到 3 个账户上限</strong>
                        <small>移除一个账户后即可连接新的邮箱。</small>
                      </span>
                    </div>
                  ) : (
                    <AccountSetupForm
                      presets={accountPresets}
                      status={null}
                      submitStatus={accountSubmitStatus}
                      error={accountError}
                      onSubmit={onConfigureAccount}
                      onGoogle={onConnectGoogle}
                    />
                  )}
                </div>
              </section>
            ) : null}

            {activeSection === "features" ? (
              <section className="settings-page" aria-labelledby="settings-features-title">
                <header className="settings-page__heading">
                  <p className="eyebrow">PREFERENCES</p>
                  <h3 id="settings-features-title">功能设定</h3>
                  <p>控制后台同步、新邮件通知、邮件图片和系统启动行为。</p>
                </header>

                <div className="settings-preference-card">
                  <label className="settings-preference-row" htmlFor="settings-sync-interval">
                    <span>
                      <strong>实时同步</strong>
                      <small>优先使用服务器推送；按此间隔完整校准删除和状态变化。</small>
                    </span>
                    <SettingsSelect
                      id="settings-sync-interval"
                      label="完整校准间隔"
                      value={value.pollingIntervalMinutes}
                      onChange={(event) =>
                        setValue((current) => ({
                          ...current,
                          pollingIntervalMinutes: Number(event.target.value),
                        }))
                      }
                    >
                      <option value={1}>1 分钟</option>
                      <option value={3}>3 分钟</option>
                      <option value={5}>5 分钟</option>
                    </SettingsSelect>
                  </label>

                  <label className="settings-preference-row settings-preference-row--toggle">
                    <span>
                      <strong>桌面通知</strong>
                      <small>同步到新邮件时显示 Mine Mail 右下角通知卡片；手动刷新同样生效。</small>
                    </span>
                    <input
                      type="checkbox"
                      checked={value.notificationsEnabled}
                      onChange={(event) =>
                        setValue((current) => ({
                          ...current,
                          notificationsEnabled: event.target.checked,
                        }))
                      }
                    />
                  </label>

                  <label className="settings-preference-row settings-preference-row--toggle">
                    <span>
                      <strong>前台也提醒</strong>
                      <small>正在使用 Mine Mail 时也显示新邮件弹窗。</small>
                    </span>
                    <input
                      type="checkbox"
                      checked={value.foregroundNotificationsEnabled}
                      disabled={!value.notificationsEnabled}
                      onChange={(event) =>
                        setValue((current) => ({
                          ...current,
                          foregroundNotificationsEnabled: event.target.checked,
                        }))
                      }
                    />
                  </label>

                  <div className="settings-preference-row">
                    <span>
                      <strong>通知声音</strong>
                      <small>新邮件通知出现时播放所选系统提示音。</small>
                    </span>
                    <span className="settings-notification-sound-control">
                      <input
                        type="checkbox"
                        aria-label="启用通知声音"
                        checked={value.notificationSoundEnabled}
                        disabled={!value.notificationsEnabled}
                        onChange={(event) =>
                          setValue((current) => ({
                            ...current,
                            notificationSoundEnabled: event.target.checked,
                          }))
                        }
                      />
                      <SettingsSelect
                        id="settings-notification-sound"
                        label="通知声音类型"
                        value={value.notificationSound}
                        disabled={
                          !value.notificationsEnabled ||
                          !value.notificationSoundEnabled
                        }
                        onChange={(event) =>
                          setValue((current) => ({
                            ...current,
                            notificationSound: event.target.value,
                          }))
                        }
                      >
                        {notificationSoundOptions.map((option) => (
                          <option key={option.id} value={option.id}>
                            {option.label}
                          </option>
                        ))}
                      </SettingsSelect>
                    </span>
                  </div>

                  <div className="settings-preference-row">
                    <span>
                      <span className="settings-preference-row__title">
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
                      </span>
                      <small>控制 HTML 邮件是否连接外部图片服务器。</small>
                    </span>
                    <SettingsSelect
                      id="settings-remote-images"
                      label="远程图片加载方式"
                      value={value.remoteImageMode}
                      onChange={(event) =>
                        setValue((current) => ({
                          ...current,
                          remoteImageMode: event.target.value,
                        }))
                      }
                    >
                      {remoteImageOptions.map((option) => (
                        <option key={option.id} value={option.id}>
                          {option.label}
                        </option>
                      ))}
                    </SettingsSelect>
                  </div>

                  <label className="settings-preference-row settings-preference-row--toggle">
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
                </div>
              </section>
            ) : null}

            {activeSection === "version" ? (
              <section className="settings-page" aria-labelledby="settings-version-title">
                <header className="settings-page__heading">
                  <p className="eyebrow">ABOUT</p>
                  <h3 id="settings-version-title">版本</h3>
                  <p>查看当前安装版本，并为后续更新功能预留入口。</p>
                </header>

                <div className="settings-version-card">
                  <span className="settings-version-card__mark">
                    <GearSix size={34} weight="duotone" />
                  </span>
                  <span className="settings-version-card__copy">
                    <small>MINE MAIL FOR DESKTOP</small>
                    <strong>v0.0.1</strong>
                    <span>当前安装版本</span>
                  </span>
                  <button type="button" className="secondary-button" disabled>
                    检查更新
                  </button>
                </div>
                <p className="settings-version-note">自动检查更新将在后续版本中提供。</p>
              </section>
            ) : null}

            {saveStatus === "error" ? (
              <p className="settings-error" role="alert">设置没有保存，请重试。</p>
            ) : null}
          </div>

          <footer className="settings-footer">
            <button
              type="button"
              className="secondary-button"
              onClick={onClose}
              disabled={saveStatus === "saving"}
            >
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
        </div>
      </section>
    </div>
  );
}
