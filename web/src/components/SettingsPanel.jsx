import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowSquareOut,
  ArrowsLeftRight,
  CaretRight,
  DotsThree,
  DownloadSimple,
  EnvelopeSimple,
  Info,
  MicrosoftOutlookLogo,
  NotePencil,
  Plus,
  Question,
  SlidersHorizontal,
  Trash,
  UserCircle,
  X,
} from "@phosphor-icons/react";
import { appUpdateApi } from "../services/appUpdate.js";
import { AccountRemovalDialog } from "./AccountRemovalDialog.jsx";
import { AccountSetupForm } from "./AccountSetup.jsx";
import { BrandLogo } from "./BrandLogo.jsx";
import { CredentialWarning } from "./CredentialWarning.jsx";
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
    icon: UserCircle,
  },
  {
    id: "features",
    label: "功能设定",
    icon: SlidersHorizontal,
  },
  {
    id: "version",
    label: "关于 Mine Mail",
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

const productLinks = [
  {
    label: "隐私政策",
    description: "了解 Gmail 数据、本地缓存和凭据的处理方式",
    url: "https://minemail.tantless.online/privacy/",
  },
  {
    label: "服务条款",
    description: "查看使用 Mine Mail 时适用的条款",
    url: "https://minemail.tantless.online/terms/",
  },
  {
    label: "数据删除指南",
    description: "了解如何撤销授权并删除本地数据",
    url: "https://minemail.tantless.online/data-deletion/",
  },
];

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
      remark: accountStatus.remark || null,
      credentialAvailable: accountStatus.credentialAvailable,
      credentialInvalid: accountStatus.credentialInvalid,
    },
  ];
}

function accountDisplayName(account) {
  return account?.remark?.trim() || account?.email || "邮箱账户";
}

function errorMessage(error, fallback) {
  return error instanceof Error && error.message ? error.message : fallback;
}

function displayVersion(version) {
  return `v${String(version || "0.0.0").replace(/^v/i, "")}`;
}

function updateErrorMessage(error) {
  const message = error instanceof Error ? error.message : String(error || "");
  if (/404|latest\.json|not found/i.test(message)) {
    return "最新 Release 暂未提供签名更新信息，请稍后重试。";
  }
  return "检查更新失败，请确认网络连接后重试。";
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
  onSaveAccountRemark,
  onRemoveAccount,
  onOpenExternalLink,
  accountAvatarFor,
  onSetAccountAvatar,
  onRemoveAccountAvatar,
  focusTarget,
  updateClient = appUpdateApi,
}) {
  const addAccountRequested =
    typeof focusTarget === "string" && focusTarget.startsWith("account-form");
  const repairAccountRequested =
    typeof focusTarget === "string" &&
    focusTarget.startsWith("account-repair");
  const [value, setValue] = useState(settings);
  const [activeSection, setActiveSection] = useState("account");
  const [accountFlow, setAccountFlow] = useState(
    addAccountRequested || repairAccountRequested ? "providers" : "overview",
  );
  const [selectedProvider, setSelectedProvider] = useState(
    repairAccountRequested ? accountStatus?.provider || null : null,
  );
  const [repairingAccount, setRepairingAccount] = useState(
    repairAccountRequested,
  );
  const [accountMenu, setAccountMenu] = useState(null);
  const [pendingAccountRemoval, setPendingAccountRemoval] = useState(null);
  const [editingAccountRemark, setEditingAccountRemark] = useState(null);
  const [accountRemarkValue, setAccountRemarkValue] = useState("");
  const [accountRemarkError, setAccountRemarkError] = useState(null);
  const [isAccountRemarkSaving, setIsAccountRemarkSaving] = useState(false);
  const [appVersion, setAppVersion] = useState(
    updateClient.bundledVersion || "0.0.0",
  );
  const [updateStatus, setUpdateStatus] = useState("idle");
  const [updateMessage, setUpdateMessage] = useState(null);
  const [availableUpdate, setAvailableUpdate] = useState(null);
  const [updateProgress, setUpdateProgress] = useState(null);
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
    let active = true;
    void updateClient
      .getCurrentVersion()
      .then((version) => {
        if (active && version) setAppVersion(version);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [updateClient]);

  useEffect(() => {
    const addRequested =
      typeof focusTarget === "string" && focusTarget.startsWith("account-form");
    const repairRequested =
      typeof focusTarget === "string" &&
      focusTarget.startsWith("account-repair");
    if (!addRequested && !repairRequested) return;
    setActiveSection("account");
    setAccountFlow("providers");
    setSelectedProvider(repairRequested ? accountStatus?.provider || null : null);
    setRepairingAccount(repairRequested);
  }, [accountStatus?.provider, focusTarget]);

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
    setRepairingAccount(false);
  }, [accountFlow, accountSubmitStatus]);

  useEffect(() => {
    scrollRef.current?.scrollTo?.({ top: 0, behavior: "smooth" });
    setAccountMenu(null);
    setPendingAccountRemoval(null);
    setEditingAccountRemark(null);
    setAccountRemarkError(null);
  }, [accountFlow, activeSection, selectedProvider]);

  useEffect(() => {
    if (!pendingAccountRemoval && !editingAccountRemark && !availableUpdate) {
      return undefined;
    }

    const closeOnEscape = (event) => {
      if (
        event.key === "Escape" &&
        accountSubmitStatus !== "saving" &&
        !isAccountRemarkSaving &&
        updateStatus !== "installing"
      ) {
        setPendingAccountRemoval(null);
        setEditingAccountRemark(null);
        if (availableUpdate) {
          setAvailableUpdate(null);
          setUpdateStatus("idle");
          setUpdateMessage(
            `已暂缓 ${displayVersion(availableUpdate.version)} 更新。`,
          );
        }
      }
    };

    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [
    accountSubmitStatus,
    editingAccountRemark,
    isAccountRemarkSaving,
    pendingAccountRemoval,
    availableUpdate,
    updateStatus,
  ]);

  const updateSettings = (updater) => {
    const next = typeof updater === "function" ? updater(value) : updater;
    setValue(next);
    void onSave(next);
  };

  const openAccountOverview = () => {
    setAccountFlow("overview");
    setSelectedProvider(null);
    setRepairingAccount(false);
  };

  const openAddAccount = () => {
    if (accountStatus?.canAddAccount === false) return;
    setActiveSection("account");
    setAccountFlow("providers");
    setSelectedProvider(null);
    setRepairingAccount(false);
  };

  const openAccountRemarkEditor = (account) => {
    setAccountMenu(null);
    setPendingAccountRemoval(null);
    setEditingAccountRemark(account);
    setAccountRemarkValue(account.remark || "");
    setAccountRemarkError(null);
  };

  const saveAccountRemark = async () => {
    if (!editingAccountRemark || isAccountRemarkSaving) return;
    setIsAccountRemarkSaving(true);
    setAccountRemarkError(null);
    try {
      await onSaveAccountRemark(
        editingAccountRemark.accountId,
        accountRemarkValue.trim(),
      );
      setEditingAccountRemark(null);
    } catch (error) {
      setAccountRemarkError(
        errorMessage(error, "邮箱备注没有保存，请重试。"),
      );
    } finally {
      setIsAccountRemarkSaving(false);
    }
  };

  const checkForUpdate = async () => {
    if (!updateClient.isSupported || ["checking", "installing"].includes(updateStatus)) {
      return;
    }
    setUpdateStatus("checking");
    setUpdateMessage(null);
    setUpdateProgress(null);
    try {
      const result = await updateClient.checkForUpdate();
      if (result.currentVersion) setAppVersion(result.currentVersion);
      if (result.status === "available") {
        setAvailableUpdate(result);
        setUpdateStatus("available");
        return;
      }
      if (result.status === "up-to-date") {
        setUpdateStatus("up-to-date");
        setUpdateMessage("已是最新版本。");
        return;
      }
      setUpdateStatus("unsupported");
      setUpdateMessage("请在 Mine Mail 桌面应用中检查更新。");
    } catch (error) {
      setUpdateStatus("error");
      setUpdateMessage(updateErrorMessage(error));
    }
  };

  const installAvailableUpdate = async () => {
    if (!availableUpdate || updateStatus === "installing") return;
    let downloaded = 0;
    let total = null;
    setUpdateStatus("installing");
    setUpdateMessage(null);
    setUpdateProgress({ stage: "starting", downloaded, total, percent: null });
    try {
      await updateClient.installUpdate(availableUpdate, (event) => {
        if (event.event === "Started") {
          total = event.data.contentLength || null;
          setUpdateProgress({
            stage: "downloading",
            downloaded,
            total,
            percent: total ? 0 : null,
          });
          return;
        }
        if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setUpdateProgress({
            stage: "downloading",
            downloaded,
            total,
            percent: total
              ? Math.min(100, Math.round((downloaded / total) * 100))
              : null,
          });
          return;
        }
        if (event.event === "Finished") {
          setUpdateProgress({
            stage: "installing",
            downloaded,
            total,
            percent: 100,
          });
        }
      });
      setAvailableUpdate(null);
      setUpdateStatus("installed");
      setUpdateMessage("更新已安装，正在重新启动 Mine Mail…");
    } catch {
      setUpdateStatus("error");
      setUpdateProgress(null);
      setUpdateMessage("更新没有安装，当前版本不会受到影响，请重试。");
    }
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
          <IconButton
            className="settings-close"
            label="关闭设置"
            onClick={onClose}
            disabled={updateStatus === "installing"}
          >
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
                      const displayName = accountDisplayName(connectedAccount);
                      const providerLabel =
                        providerNames[connectedAccount.provider] || "邮箱账户";
                      const credentialIssue =
                        connectedAccount.credentialInvalid ||
                        connectedAccount.credentialAvailable === false;
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
                            label={displayName}
                            customSrc={customAvatar}
                            onSelectFile={(file) =>
                              onSetAccountAvatar(connectedAccount.email, file)
                            }
                            onRemove={() =>
                              onRemoveAccountAvatar(connectedAccount.email)
                            }
                          />
                          <span className="settings-account-card__copy">
                            <strong>{displayName}</strong>
                            <small>
                              {connectedAccount.remark
                                ? `${connectedAccount.email} · ${providerLabel}`
                                : providerLabel}
                            </small>
                            {credentialIssue ? <CredentialWarning /> : null}
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
                                  onClick={() =>
                                    openAccountRemarkEditor(connectedAccount)
                                  }
                                >
                                  <NotePencil size={15} />
                                  {connectedAccount.remark ? "编辑备注" : "添加备注"}
                                </button>
                                <button
                                  type="button"
                                  role="menuitem"
                                  data-tone="danger"
                                  onClick={() => {
                                    setAccountMenu(null);
                                    setPendingAccountRemoval(connectedAccount);
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
                      <strong>{accountDisplayName(activeAccount)}</strong>
                      {activeAccount.remark ? (
                        <small>{activeAccount.email}</small>
                      ) : null}
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
                      <small>服务器推送优先；按此间隔校准删除、收藏与状态变化。</small>
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
                <IconButton
                  label={repairingAccount ? "返回账户设置" : "返回选择邮箱服务商"}
                  onClick={
                    repairingAccount
                      ? openAccountOverview
                      : () => setSelectedProvider(null)
                  }
                >
                  <ArrowLeft size={18} />
                </IconButton>
                <span>
                  <p className="eyebrow">
                    {repairingAccount ? "修复账户" : "添加账户"}
                  </p>
                  <h3 id="connect-title">连接 {providerNames[selectedProvider] || "邮箱"}</h3>
                  <p>{providerDescriptions[selectedProvider]}</p>
                </span>
              </header>

              <div className="settings-account-setup">
                <AccountSetupForm
                  key={selectedProvider}
                  presets={accountPresets}
                  status={repairingAccount ? accountStatus : null}
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
                </span>
              </header>

              <div className="settings-preference-card">
                <label className="settings-preference-row settings-preference-row--toggle">
                  <span>
                    <strong>桌面通知</strong>
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
                  <strong>{displayVersion(appVersion)}</strong>
                  <span>当前安装版本</span>
                </span>
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => void checkForUpdate()}
                  disabled={
                    !updateClient.isSupported ||
                    ["checking", "installing", "available"].includes(updateStatus)
                  }
                >
                  {updateStatus === "checking"
                    ? "正在检查…"
                    : updateStatus === "installing"
                      ? "正在更新…"
                      : "检查更新"}
                </button>
              </div>
              <p
                className="settings-version-note"
                data-tone={updateStatus === "error" ? "danger" : undefined}
                role={updateStatus === "error" ? "alert" : undefined}
                aria-live="polite"
              >
                {updateMessage ||
                  (updateClient.isSupported
                    ? "更新来自 GitHub Releases。"
                    : "浏览器预览不执行更新，请使用 Mine Mail 桌面应用。")}
              </p>
            </section>
          ) : null}

          {activeSection === "version" ? (
            <section className="settings-legal-card" aria-labelledby="settings-legal-title">
              <header>
                <strong id="settings-legal-title">隐私与数据</strong>
                <small>这些说明会在系统浏览器中打开，方便保存和查阅。</small>
              </header>
              <div>
                {productLinks.map((link) => (
                  <button
                    type="button"
                    className="settings-legal-link"
                    key={link.url}
                    onClick={() => onOpenExternalLink(link.url)}
                  >
                    <span>
                      <strong>{link.label}</strong>
                      <small>{link.description}</small>
                    </span>
                    <ArrowSquareOut size={17} />
                  </button>
                ))}
              </div>
            </section>
          ) : null}

          {saveStatus === "error" ? (
            <p className="settings-error" role="alert">设置没有保存，请重试。</p>
          ) : null}
        </div>
      </div>

      {availableUpdate ? (
        <div
          className="confirm-layer"
          onMouseDown={(event) => {
            if (
              event.target === event.currentTarget &&
              updateStatus !== "installing"
            ) {
              setAvailableUpdate(null);
              setUpdateStatus("idle");
              setUpdateMessage(
                `已暂缓 ${displayVersion(availableUpdate.version)} 更新。`,
              );
            }
          }}
        >
          <section
            className="confirm-dialog update-confirm-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="update-confirm-title"
            aria-describedby="update-confirm-description"
          >
            <header>
              <span className="confirm-dialog__icon">
                <DownloadSimple size={22} weight="duotone" />
              </span>
              <IconButton
                label="暂不更新"
                onClick={() => {
                  setAvailableUpdate(null);
                  setUpdateStatus("idle");
                  setUpdateMessage(
                    `已暂缓 ${displayVersion(availableUpdate.version)} 更新。`,
                  );
                }}
                disabled={updateStatus === "installing"}
              >
                <X size={18} />
              </IconButton>
            </header>
            <h2 id="update-confirm-title">
              发现 Mine Mail {displayVersion(availableUpdate.version)}
            </h2>
            <p id="update-confirm-description">
              当前为 {displayVersion(appVersion)}。是否下载并安装来自 GitHub
              Release 的签名更新？
            </p>
            {availableUpdate.notes ? (
              <div className="update-confirm-dialog__notes">
                <small>更新说明</small>
                <p>{availableUpdate.notes}</p>
              </div>
            ) : null}
            {updateStatus === "installing" ? (
              <div className="update-confirm-dialog__progress" aria-live="polite">
                <span>
                  {updateProgress?.stage === "installing"
                    ? "正在启动安装程序…"
                    : updateProgress?.percent != null
                      ? `正在下载… ${updateProgress.percent}%`
                      : "正在准备下载…"}
                </span>
                <progress
                  aria-label="更新下载进度"
                  max="100"
                  value={updateProgress?.percent ?? undefined}
                />
                <small>安装时应用会自动退出；完成后将重新打开。</small>
              </div>
            ) : null}
            {updateStatus === "error" && updateMessage ? (
              <p className="settings-error" role="alert">
                {updateMessage}
              </p>
            ) : null}
            <footer>
              <button
                type="button"
                className="secondary-button"
                autoFocus
                onClick={() => {
                  setAvailableUpdate(null);
                  setUpdateStatus("idle");
                  setUpdateMessage(
                    `已暂缓 ${displayVersion(availableUpdate.version)} 更新。`,
                  );
                }}
                disabled={updateStatus === "installing"}
              >
                暂不更新
              </button>
              <button
                type="button"
                className="send-button"
                onClick={() => void installAvailableUpdate()}
                disabled={updateStatus === "installing"}
              >
                <DownloadSimple size={17} weight="bold" />
                {updateStatus === "installing" ? "正在更新…" : "下载并安装"}
              </button>
            </footer>
          </section>
        </div>
      ) : null}

      {editingAccountRemark ? (
        <div
          className="confirm-layer"
          onMouseDown={(event) => {
            if (
              event.target === event.currentTarget &&
              !isAccountRemarkSaving
            ) {
              setEditingAccountRemark(null);
            }
          }}
        >
          <form
            className="confirm-dialog account-remark-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="account-remark-title"
            noValidate
            onSubmit={(event) => {
              event.preventDefault();
              void saveAccountRemark();
            }}
          >
            <header>
              <span className="confirm-dialog__icon">
                <NotePencil size={22} weight="duotone" />
              </span>
              <IconButton
                label="关闭邮箱备注编辑"
                onClick={() => setEditingAccountRemark(null)}
                disabled={isAccountRemarkSaving}
              >
                <X size={18} />
              </IconButton>
            </header>
            <h2 id="account-remark-title">设置邮箱备注</h2>
            <p>
              备注会用于账户来源、收藏夹和新邮件通知；邮箱地址仍会同时显示。
            </p>
            <label className="settings-field account-remark-dialog__field">
              <span>备注名</span>
              <span className="settings-input-shell settings-input-shell--text inset-input-shell">
                <input
                  type="text"
                  aria-label="备注名"
                  value={accountRemarkValue}
                  maxLength={40}
                  autoComplete="off"
                  autoFocus
                  disabled={isAccountRemarkSaving}
                  placeholder="例如：工作邮箱"
                  onChange={(event) => {
                    setAccountRemarkValue(event.target.value);
                    setAccountRemarkError(null);
                  }}
                />
              </span>
              <small>留空并保存可删除备注，最多 40 个字符。</small>
            </label>
            {accountRemarkError ? (
              <p className="settings-error" role="alert">
                {accountRemarkError}
              </p>
            ) : null}
            <footer>
              <button
                type="button"
                className="secondary-button"
                onClick={() => setEditingAccountRemark(null)}
                disabled={isAccountRemarkSaving}
              >
                取消
              </button>
              <button
                type="submit"
                className="send-button"
                disabled={isAccountRemarkSaving}
              >
                {isAccountRemarkSaving ? "正在保存…" : "保存备注"}
              </button>
            </footer>
          </form>
        </div>
      ) : null}

      <AccountRemovalDialog
        account={pendingAccountRemoval}
        isRemoving={accountSubmitStatus === "saving"}
        onCancel={() => setPendingAccountRemoval(null)}
        onConfirm={(options) => {
          const accountToRemove = pendingAccountRemoval;
          setPendingAccountRemoval(null);
          if (accountToRemove) onRemoveAccount(accountToRemove, options);
        }}
      />
    </section>
  );
}
