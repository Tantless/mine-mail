import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowsLeftRight,
  CaretRight,
  DotsThree,
  EnvelopeSimple,
  Info,
  MicrosoftOutlookLogo,
  Plus,
  Question,
  SlidersHorizontal,
  Trash,
  UserCircle,
  X,
} from "@phosphor-icons/react";
import { AccountSetupForm } from "./AccountSetup.jsx";
import { BrandLogo } from "./BrandLogo.jsx";
import { IconButton } from "./IconButton.jsx";
import { EditableProfileAvatar, ProfileAvatar } from "./ProfileAvatar.jsx";
import { ThemedSelect } from "./ThemedSelect.jsx";

const remoteImageOptions = [
  { value: "automatic", label: "自动加载" },
  { value: "ask", label: "每次询问" },
  { value: "blocked", label: "始终阻止" },
];

const notificationSoundOptions = [
  { value: "mail", label: "邮件提示音" },
  { value: "default", label: "系统默认" },
  { value: "im", label: "轻柔提示" },
  { value: "reminder", label: "提醒提示" },
];

const syncIntervalOptions = [
  { value: 1, label: "1 分钟" },
  { value: 3, label: "3 分钟" },
  { value: 5, label: "5 分钟" },
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
    description: "通知、图片与启动",
    icon: SlidersHorizontal,
  },
  {
    id: "version",
    label: "关于 Mine Mail",
    description: "版本与更新",
    icon: Info,
  },
];

const fallbackProviders = [
  { id: "163", label: "163 邮箱", description: "使用 163 邮箱客户端授权码连接" },
  { id: "gmail", label: "Gmail", description: "通过 Google 安全登录" },
  { id: "outlook", label: "Outlook", description: "通过 Microsoft 账户连接", disabled: true },
  { id: "custom", label: "其他邮箱", description: "手动配置 IMAP / SMTP" },
];

const providerNames = {
  "163": "163 邮箱",
  gmail: "Gmail",
  outlook: "Outlook",
  custom: "自定义邮箱",
};

const providerDescriptions = {
  "163": "输入 163 邮箱地址，并使用客户端授权码完成连接。",
  gmail: "在系统浏览器中完成 Google OAuth 安全登录。",
  outlook: "通过 Microsoft 账户完成连接。",
  custom: "输入邮箱地址、授权信息以及 IMAP / SMTP 服务器配置。",
};

const remoteImageRisk =
  "自动加载会连接发件人的图片服务器，可能暴露邮件打开时间、IP 地址和设备信息，并让追踪像素确认邮箱处于活跃状态。";

function normalizeProvider(preset) {
  const id = preset.id ?? preset.provider ?? preset.provider_id;
  const fallback = fallbackProviders.find((provider) => provider.id === id);
  return {
    id,
    label: preset.label ?? preset.name ?? fallback?.label ?? id,
    description:
      preset.note ??
      preset.authenticationNote ??
      preset.authentication_note ??
      fallback?.description ??
      "连接邮箱服务商账户",
    disabled: Boolean(
      preset.disabled ||
        preset.availableInMvp === false ||
        preset.available_in_mvp === false ||
        id === "outlook",
    ),
  };
}

function connectedAccounts(accountStatus) {
  if (accountStatus?.accounts?.length) return accountStatus.accounts;
  if (!accountStatus?.configured || !accountStatus?.email) return [];
  return [
    {
      accountId: accountStatus.accountId || "primary",
      provider: accountStatus.provider,
      email: accountStatus.email,
    },
  ];
}

function ProviderMark({ provider }) {
  if (provider === "163") {
    return <ProfileAvatar className="settings-provider-mark" email="mail@163.com" label="163 邮箱" />;
  }
  if (provider === "gmail") {
    return <ProfileAvatar className="settings-provider-mark" email="mail@gmail.com" label="Gmail" />;
  }
  if (provider === "outlook") {
    return (
      <span className="settings-provider-mark settings-provider-mark--outlook">
        <MicrosoftOutlookLogo size={22} weight="duotone" />
      </span>
    );
  }
  return (
    <span className="settings-provider-mark settings-provider-mark--custom">
      <EnvelopeSimple size={21} weight="duotone" />
    </span>
  );
}

function SettingsSelect({ id, label, value, onValueChange, disabled = false, options }) {
  return (
    <ThemedSelect
      id={id}
      className="settings-select-wrap"
      label={label}
      value={value}
      options={options}
      onValueChange={onValueChange}
      disabled={disabled}
    />
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
  accountAvatarFor,
  onSetAccountAvatar,
  onRemoveAccountAvatar,
  focusTarget,
}) {
  const addAccountRequested =
    typeof focusTarget === "string" && focusTarget.startsWith("account-form");
  const [value, setValue] = useState(settings);
  const [activeSection, setActiveSection] = useState("account");
  const [accountFlow, setAccountFlow] = useState(
    addAccountRequested ? "providers" : "overview",
  );
  const [selectedProvider, setSelectedProvider] = useState(null);
  const [accountMenu, setAccountMenu] = useState(null);
  const scrollRef = useRef(null);
  const previousAccountSubmitStatusRef = useRef(accountSubmitStatus);

  const accounts = connectedAccounts(accountStatus);
  const maxAccounts = accountStatus?.maxAccounts || 3;
  const activeAccount =
    accounts.find(
      (account) =>
        account.accountId === (accountStatus?.activeAccountId || accountStatus?.accountId),
    ) || accounts[0];
  const providerOptions = useMemo(
    () =>
      (accountPresets?.length ? accountPresets.map(normalizeProvider) : fallbackProviders),
    [accountPresets],
  );

  useEffect(() => {
    setValue(settings);
  }, [settings]);

  useEffect(() => {
    if (!(typeof focusTarget === "string" && focusTarget.startsWith("account-form"))) return;
    setActiveSection("account");
    setAccountFlow("providers");
    setSelectedProvider(null);
  }, [focusTarget]);

  useEffect(() => {
    const previousStatus = previousAccountSubmitStatusRef.current;
    previousAccountSubmitStatusRef.current = accountSubmitStatus;
    if (
      previousStatus !== "saving" ||
      accountSubmitStatus !== "saved" ||
      accountFlow === "overview"
    ) {
      return;
    }
    setAccountFlow("overview");
    setSelectedProvider(null);
  }, [accountFlow, accountSubmitStatus]);

  useEffect(() => {
    scrollRef.current?.scrollTo?.({ top: 0, behavior: "smooth" });
    setAccountMenu(null);
  }, [accountFlow, activeSection, selectedProvider]);

  const updateSettings = (updater) => {
    const next = typeof updater === "function" ? updater(value) : updater;
    setValue(next);
    void onSave(next);
  };

  const openAccountOverview = () => {
    setAccountFlow("overview");
    setSelectedProvider(null);
  };

  const openAddAccount = () => {
    if (accountStatus?.canAddAccount === false) return;
    setActiveSection("account");
    setAccountFlow("providers");
    setSelectedProvider(null);
  };

  const saveStateLabel =
    saveStatus === "saving"
      ? "正在保存…"
      : saveStatus === "saved"
        ? "已自动保存"
        : saveStatus === "error"
          ? "保存失败"
          : "";

  return (
    <section className="settings-workspace" aria-labelledby="settings-title">
      <aside className="settings-sidebar">
        <div className="settings-sidebar__heading">
          <span>MINE MAIL</span>
          <h2 id="settings-title">设置</h2>
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
                onClick={() => {
                  setActiveSection(item.id);
                  if (item.id !== "account") openAccountOverview();
                }}
              >
                <span className="settings-nav__icon">
                  <MenuIcon size={19} weight={selected ? "fill" : "regular"} />
                </span>
                <span className="settings-nav__copy">
                  <strong>{item.label}</strong>
                  <small>{item.description}</small>
                </span>
              </button>
            );
          })}
        </nav>

        <p className="settings-sidebar__note">偏好设置会自动保存在这台设备上。</p>
      </aside>

      <div className="settings-content">
        <header className="settings-topbar">
          <span
            className="settings-save-state"
            data-tone={saveStatus === "error" ? "danger" : undefined}
            aria-live="polite"
          >
            {saveStateLabel}
          </span>
          <IconButton className="settings-close" label="关闭设置" onClick={onClose}>
            <X size={18} />
          </IconButton>
        </header>

        <div ref={scrollRef} className="settings-scroll vertical-scroll-surface">
          {activeSection === "account" && accountFlow === "overview" ? (
            <section className="settings-page" aria-labelledby="settings-account-title">
              <header className="settings-page__heading settings-page__heading--with-action">
                <span>
                  <p className="eyebrow">ACCOUNT</p>
                  <h3 id="settings-account-title">账户与同步</h3>
                  <p>管理已连接的邮箱、当前账户和同步身份。</p>
                </span>
                <button
                  type="button"
                  className="send-button settings-add-account"
                  onClick={openAddAccount}
                  disabled={accountStatus?.canAddAccount === false}
                >
                  <Plus size={16} weight="bold" />
                  添加账户
                </button>
              </header>

              <div className="settings-subsection settings-account-section">
                <div className="settings-subsection__heading">
                  <span>
                    <strong>已连接的邮箱</strong>
                    <small>
                      最多连接 {maxAccounts} 个账户，当前已连接 {accounts.length} 个。
                    </small>
                  </span>
                </div>

                {accounts.length ? (
                  <div className="settings-account-list">
                    {accounts.map((connectedAccount) => {
                      const active =
                        connectedAccount.accountId ===
                        (accountStatus.activeAccountId || accountStatus.accountId);
                      const customAvatar = accountAvatarFor?.(connectedAccount.email);
                      return (
                        <div
                          className="settings-account-card"
                          data-active={active}
                          key={connectedAccount.accountId}
                        >
                          <EditableProfileAvatar
                            className="settings-account-card__avatar-picker"
                            avatarClassName="settings-account-card__avatar"
                            email={connectedAccount.email}
                            label={connectedAccount.email}
                            customSrc={customAvatar}
                            onSelectFile={(file) =>
                              onSetAccountAvatar(connectedAccount.email, file)
                            }
                            onRemove={() =>
                              onRemoveAccountAvatar(connectedAccount.email)
                            }
                          />
                          <span className="settings-account-card__copy">
                            <strong>{connectedAccount.email}</strong>
                            <small>{providerNames[connectedAccount.provider] || "邮箱账户"}</small>
                          </span>
                          {active ? (
                            <span className="settings-current-chip">当前</span>
                          ) : (
                            <IconButton
                              className="settings-account-action"
                              label={`切换到 ${connectedAccount.email}`}
                              title="设为当前账户"
                              onClick={() => onSwitchAccount(connectedAccount.accountId)}
                              disabled={accountSubmitStatus === "saving"}
                            >
                              <ArrowsLeftRight size={17} />
                            </IconButton>
                          )}
                          <span className="settings-account-menu-wrap">
                            <IconButton
                              className="settings-account-action"
                              label={`管理 ${connectedAccount.email}`}
                              title="更多账户操作"
                              aria-expanded={accountMenu === connectedAccount.accountId}
                              onClick={() =>
                                setAccountMenu((current) =>
                                  current === connectedAccount.accountId
                                    ? null
                                    : connectedAccount.accountId,
                                )
                              }
                            >
                              <DotsThree size={20} weight="bold" />
                            </IconButton>
                            {accountMenu === connectedAccount.accountId ? (
                              <span className="settings-account-menu" role="menu">
                                <button
                                  type="button"
                                  role="menuitem"
                                  onClick={() => {
                                    setAccountMenu(null);
                                    onRemoveAccount(connectedAccount);
                                  }}
                                >
                                  <Trash size={15} />
                                  移除账户
                                </button>
                              </span>
                            ) : null}
                          </span>
                        </div>
                      );
                    })}
                  </div>
                ) : (
                  <div className="settings-account-empty">
                    <UserCircle size={27} weight="duotone" />
                    <span>
                      <strong>尚未连接邮箱</strong>
                      <small>选择“添加账户”即可开始。</small>
                    </span>
                  </div>
                )}
              </div>

              {accountStatus?.canAddAccount === false ? (
                <p className="settings-limit-note">已达到三个账户上限；移除一个账户后可继续添加。</p>
              ) : null}

              {activeAccount ? (
                <section className="settings-overview-section" aria-labelledby="sending-account-title">
                  <div className="settings-subsection__heading">
                    <span>
                      <strong id="sending-account-title">当前发件账户</strong>
                    </span>
                  </div>
                  <div className="settings-inline-account">
                    <ProviderMark provider={activeAccount.provider} />
                    <span>
                      <strong>{activeAccount.email}</strong>
                      <small>新邮件将默认使用当前账户发出。</small>
                    </span>
                  </div>
                </section>
              ) : null}

              <section className="settings-overview-section" aria-labelledby="sync-settings-title">
                <div className="settings-subsection__heading">
                  <span>
                    <strong id="sync-settings-title">同步设置</strong>
                  </span>
                </div>
                <div className="settings-preference-card settings-preference-card--single">
                  <label className="settings-preference-row" htmlFor="settings-sync-interval">
                    <span>
                      <strong>完整校准间隔</strong>
                      <small>服务器推送优先；按此间隔校准删除、星标与状态变化。</small>
                    </span>
                    <SettingsSelect
                      id="settings-sync-interval"
                      label="完整校准间隔"
                      value={value.pollingIntervalMinutes}
                      options={syncIntervalOptions}
                      onValueChange={(pollingIntervalMinutes) =>
                        updateSettings((current) => ({
                          ...current,
                          pollingIntervalMinutes,
                        }))
                      }
                    />
                  </label>
                </div>
              </section>
            </section>
          ) : null}

          {activeSection === "account" && accountFlow === "providers" && !selectedProvider ? (
            <section className="settings-page settings-page--flow" aria-labelledby="provider-title">
              <header className="settings-flow-heading">
                <IconButton label="返回账户设置" onClick={openAccountOverview}>
                  <ArrowLeft size={18} />
                </IconButton>
                <span>
                  <p className="eyebrow">添加账户</p>
                  <h3 id="provider-title">选择邮箱服务商</h3>
                  <p>选择你的邮箱服务商以开始连接。</p>
                </span>
              </header>

              <div className="settings-provider-list">
                {providerOptions.map((provider) => (
                  <button
                    key={provider.id}
                    type="button"
                    disabled={provider.disabled}
                    onClick={() => setSelectedProvider(provider.id)}
                  >
                    <ProviderMark provider={provider.id} />
                    <span>
                      <strong>{provider.label}</strong>
                      <small>{provider.description}</small>
                    </span>
                    {provider.disabled ? (
                      <small className="settings-provider-status">即将支持</small>
                    ) : (
                      <CaretRight size={17} aria-hidden="true" />
                    )}
                  </button>
                ))}
              </div>
            </section>
          ) : null}

          {activeSection === "account" && accountFlow === "providers" && selectedProvider ? (
            <section className="settings-page settings-page--flow" aria-labelledby="connect-title">
              <header className="settings-flow-heading">
                <IconButton label="返回选择邮箱服务商" onClick={() => setSelectedProvider(null)}>
                  <ArrowLeft size={18} />
                </IconButton>
                <span>
                  <p className="eyebrow">添加账户</p>
                  <h3 id="connect-title">连接 {providerNames[selectedProvider] || "邮箱"}</h3>
                  <p>{providerDescriptions[selectedProvider]}</p>
                </span>
              </header>

              <div className="settings-account-setup">
                <AccountSetupForm
                  key={selectedProvider}
                  presets={accountPresets}
                  status={null}
                  submitStatus={accountSubmitStatus}
                  error={accountError}
                  initialProvider={selectedProvider}
                  showProviderPicker={false}
                  onSubmit={onConfigureAccount}
                  onGoogle={onConnectGoogle}
                />
              </div>
            </section>
          ) : null}

          {activeSection === "features" ? (
            <section className="settings-page" aria-labelledby="settings-features-title">
              <header className="settings-page__heading">
                <span>
                  <p className="eyebrow">PREFERENCES</p>
                  <h3 id="settings-features-title">功能设定</h3>
                  <p>控制后台同步、新邮件通知、邮件图片和系统启动行为。</p>
                </span>
              </header>

              <div className="settings-preference-card">
                <label className="settings-preference-row settings-preference-row--toggle">
                  <span>
                    <strong>桌面通知</strong>
                    <small>新邮件到达时显示 Mine Mail 的主题通知卡片。</small>
                  </span>
                  <input
                    type="checkbox"
                    checked={value.notificationsEnabled}
                    onChange={(event) =>
                      updateSettings((current) => ({
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
                      updateSettings((current) => ({
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
                        updateSettings((current) => ({
                          ...current,
                          notificationSoundEnabled: event.target.checked,
                        }))
                      }
                    />
                    <SettingsSelect
                      id="settings-notification-sound"
                      label="通知声音类型"
                      value={value.notificationSound}
                      options={notificationSoundOptions}
                      disabled={!value.notificationsEnabled || !value.notificationSoundEnabled}
                      onValueChange={(notificationSound) =>
                        updateSettings((current) => ({
                          ...current,
                          notificationSound,
                        }))
                      }
                    />
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
                        <span id="remote-image-risk" className="settings-help__tooltip" role="tooltip">
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
                    options={remoteImageOptions}
                    onValueChange={(remoteImageMode) =>
                      updateSettings((current) => ({
                        ...current,
                        remoteImageMode,
                      }))
                    }
                  />
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
                      updateSettings((current) => ({
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
                <span>
                  <p className="eyebrow">ABOUT</p>
                  <h3 id="settings-version-title">关于 Mine Mail</h3>
                  <p>查看当前安装版本与更新状态。</p>
                </span>
              </header>

              <div className="settings-version-card">
                <span className="settings-version-card__mark">
                  <BrandLogo />
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
      </div>
    </section>
  );
}
