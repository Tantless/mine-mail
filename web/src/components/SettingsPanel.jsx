import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowsLeftRight,
  CaretRight,
  DotsThree,
  DownloadSimple,
  EnvelopeSimple,
  FolderOpen,
  HardDrives,
  Info,
  MicrosoftOutlookLogo,
  NotePencil,
  Plus,
  Question,
  Robot,
  SlidersHorizontal,
  StopCircle,
  Trash,
  UserCircle,
  X,
} from "@phosphor-icons/react";
import { appStorageApi } from "../services/appStorage.js";
import { appUpdateApi } from "../services/appUpdate.js";
import { mailApi } from "../services/mailApi.js";
import {
  displayAppVersion,
  useAppUpdate,
} from "../hooks/useAppUpdate.js";
import { AccountRemovalDialog } from "./AccountRemovalDialog.jsx";
import { AccountSetupForm } from "./AccountSetup.jsx";
import { AgentSettings } from "./AgentSettings.jsx";
import { AuthorizationGuide } from "./AuthorizationGuide.jsx";
import { BrandLogo } from "./BrandLogo.jsx";
import {
  ConfirmDialogStatus,
  useConfirmDialogFocus,
} from "./ConfirmDialogPrimitives.jsx";
import { CredentialWarning } from "./CredentialWarning.jsx";
import { IconButton } from "./IconButton.jsx";
import { EditableProfileAvatar, ProfileAvatar } from "./ProfileAvatar.jsx";
import { ThemedSelect } from "./ThemedSelect.jsx";
import { TooltipTarget } from "./Tooltip.jsx";
import { useSlidingSelection } from "../hooks/useSlidingSelection.js";
import { userFacingErrorMessage } from "../utils/userFacingError.js";

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
    id: "agent",
    label: "Agent 配置",
    icon: Robot,
  },
  {
    id: "version",
    label: "关于 Mine Mail",
    icon: Info,
  },
];

const fallbackProviders = [
  { id: "163", label: "163 邮箱", description: "使用 163 邮箱客户端授权码连接" },
  { id: "qq", label: "QQ 邮箱", description: "使用 QQ 邮箱授权码连接" },
  { id: "gmail", label: "Gmail", description: "通过 Google 安全登录" },
  { id: "custom", label: "其他邮箱", description: "手动配置 IMAP / SMTP" },
];

const providerNames = {
  "163": "163 邮箱",
  qq: "QQ 邮箱",
  gmail: "Gmail",
  outlook: "Outlook",
  custom: "自定义邮箱",
};

const providerDescriptions = {
  "163": "输入 163 邮箱账号，并使用客户端授权码完成连接。",
  qq: "输入 QQ 邮箱账号，并使用 QQ 邮箱生成的授权码完成连接。",
  gmail: "在系统浏览器中完成 Google OAuth 安全登录。",
  custom: "输入邮箱地址、授权信息以及 IMAP / SMTP 服务器配置。",
};

const legacyOutlookNotice =
  "当前版本尚未支持 Outlook 的 Microsoft OAuth / Modern Auth。已缓存邮件仍可阅读，但此账户不能重新连接，也不能新建 Outlook 账户。";

const remoteImageRisk =
  "自动加载会连接发件人的图片服务器，可能暴露邮件打开时间、IP 地址和设备信息，并让追踪像素确认邮箱处于活跃状态。";

const productLinks = [
  {
    label: "隐私政策",
    url: "https://minemail.tantless.online/privacy/",
  },
  {
    label: "服务条款",
    url: "https://minemail.tantless.online/terms/",
  },
  {
    label: "数据删除指南",
    url: "https://minemail.tantless.online/data-deletion/",
  },
];

const sourceRepositoryUrl = "https://github.com/Tantless/mine-mail";

const mcpToolGroups = [
  {
    label: "账户与同步",
    tools: [
      ["list_accounts", "查看可用邮箱账户"],
      ["sync_mail", "同步指定账户的邮件"],
    ],
  },
  {
    label: "检索与阅读",
    tools: [
      ["search_messages", "检索邮件与已缓存正文"],
      ["index_message_bodies", "分批补全正文检索范围"],
      ["get_message", "读取邮件正文与附件信息"],
    ],
  },
  {
    label: "附件下载",
    tools: [["download_attachment", "把收件附件保存到本机"]],
  },
  {
    label: "邮件整理",
    tools: [
      ["set_message_read", "切换已读状态"],
      ["set_message_starred", "切换星标状态"],
      ["archive_message", "归档邮件"],
      ["move_message_to_inbox", "移回收件箱"],
      ["move_message_to_trash", "移入废纸篓"],
    ],
  },
  {
    label: "草稿与附件",
    tools: [
      ["list_drafts", "查看草稿列表"],
      ["get_draft", "读取一封草稿"],
      ["create_draft", "新建草稿"],
      ["update_draft", "编辑指定版本的草稿"],
      ["delete_draft", "删除指定版本的草稿"],
      ["add_draft_attachments", "从本机添加草稿附件"],
      ["remove_draft_attachment", "移除草稿附件"],
    ],
  },
  {
    label: "回复与发送",
    tools: [
      ["create_reply_draft", "创建回复草稿"],
      ["create_forward_draft", "创建转发草稿"],
      ["send_draft", "发送确认过的草稿版本"],
    ],
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
  return userFacingErrorMessage(error, fallback);
}

function formatStorageBytes(bytes) {
  const value = Number(bytes) || 0;
  if (value < 1024) return `${value} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let size = value / 1024;
  let unit = units[0];
  for (let index = 1; index < units.length && size >= 1024; index += 1) {
    size /= 1024;
    unit = units[index];
  }
  return `${size >= 100 ? size.toFixed(0) : size.toFixed(1)} ${unit}`;
}

function storageCompositionLabel(categories) {
  const entries = (categories || []).map(
    (category) => `${category.label} ${formatStorageBytes(category.bytes)}`,
  );
  return entries.length
    ? `本地数据空间占用构成：${entries.join("，")}`
    : "本地数据空间占用构成：暂无数据";
}

function storageLocationLabel(kind) {
  if (kind === "install_directory") return "随应用安装位置";
  if (kind === "custom") return "自定义位置";
  return "Windows 默认位置";
}

function ProviderMark({ provider }) {
  if (provider === "163") {
    return <ProfileAvatar className="settings-provider-mark" email="mail@163.com" label="163 邮箱" />;
  }
  if (provider === "gmail") {
    return <ProfileAvatar className="settings-provider-mark" email="mail@gmail.com" label="Gmail" />;
  }
  if (provider === "qq") {
    return <ProfileAvatar className="settings-provider-mark" email="mail@qq.com" label="QQ 邮箱" />;
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
  accountErrorProvider,
  onConfigureAccount,
  onConnectGoogle,
  onAccountProviderChange,
  onSwitchAccount,
  onSaveAccountRemark,
  onRemoveAccount,
  onOpenExternalLink,
  accountAvatarFor,
  onSetAccountAvatar,
  onRemoveAccountAvatar,
  focusTarget,
  updateClient = appUpdateApi,
  appUpdateController = null,
  storageClient = appStorageApi,
  agentClient = mailApi,
}) {
  const ownedAppUpdateController = useAppUpdate(updateClient, {
    enabled: !appUpdateController,
  });
  const {
    appVersion,
    availableUpdate,
    cancelDownload,
    checkForUpdate,
    closeDialog: closeUpdateDialogController,
    installAvailableUpdate,
    isDialogOpen: isUpdateDialogOpen,
    isDownloadCancellable,
    message: updateMessage,
    minimizeDialog: minimizeUpdateDialogController,
    progress: updateProgress,
    status: updateStatus,
  } = appUpdateController || ownedAppUpdateController;
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
  const [authorizationGuideProvider, setAuthorizationGuideProvider] =
    useState(null);
  const [accountMenu, setAccountMenu] = useState(null);
  const [pendingAccountRemoval, setPendingAccountRemoval] = useState(null);
  const [editingAccountRemark, setEditingAccountRemark] = useState(null);
  const [accountRemarkValue, setAccountRemarkValue] = useState("");
  const [accountRemarkError, setAccountRemarkError] = useState(null);
  const [isAccountRemarkSaving, setIsAccountRemarkSaving] = useState(false);
  const [storageStatus, setStorageStatus] = useState(null);
  const [storageState, setStorageState] = useState("idle");
  const [storageMessage, setStorageMessage] = useState(null);
  const [pendingStorageDirectory, setPendingStorageDirectory] = useState(null);
  const [isRemoteImageHelpOpen, setIsRemoteImageHelpOpen] = useState(false);
  const [isMcpHelpOpen, setIsMcpHelpOpen] = useState(false);
  const [isMcpEnablePending, setIsMcpEnablePending] = useState(false);
  const scrollRef = useRef(null);
  const settingsNavRef = useRef(null);
  const accountMenuRef = useRef(null);
  const accountMenuTriggerRef = useRef(null);
  const previousAccountSubmitStatusRef = useRef(accountSubmitStatus);
  const accountActionReturnFocusRef = useRef(null);
  const storageDialogReturnFocusRef = useRef(null);
  const updateDialogReturnFocusRef = useRef(null);
  const settingsCloseRef = useRef(null);
  const storageCancelRef = useRef(null);
  const updateCancelRef = useRef(null);
  const mcpEnableCancelRef = useRef(null);
  const mcpHelpCloseRef = useRef(null);
  const accountRemarkInputRef = useRef(null);
  const authorizationGuideButtonRef = useRef(null);
  const authorizationGuideReturnFocusRef = useRef(false);

  const accounts = connectedAccounts(accountStatus);
  const maxAccounts = accountStatus?.maxAccounts || 3;
  const activeAccount =
    accounts.find(
      (account) =>
        account.accountId === (accountStatus?.activeAccountId || accountStatus?.accountId),
    ) || accounts[0];
  const {
    motionReady: navSelectionMotionReady,
    selectionStyle: navSelectionStyle,
    selectionVisible: navSelectionVisible,
  } = useSlidingSelection({
    containerRef: settingsNavRef,
    layoutKey: activeSection,
    selectedKey: activeSection,
  });
  const providerOptions = useMemo(() => {
    const providers = accountPresets?.length
      ? accountPresets.map(normalizeProvider)
      : fallbackProviders;
    return providers.filter(
      (provider) => provider.id !== "outlook" && !provider.disabled,
    );
  }, [accountPresets]);

  useEffect(() => {
    setValue(settings);
  }, [settings]);

  useEffect(() => {
    if (activeSection !== "version") return undefined;
    let active = true;
    setStorageState("loading");
    setStorageMessage(null);
    void storageClient
      .getStatus()
      .then((status) => {
        if (!active) return;
        setStorageStatus(status);
        setStorageState("idle");
        if (status.migrationNotice) {
          setStorageMessage({
            text:
              status.migrationNotice.status === "failed"
                ? errorMessage(
                    status.migrationNotice.message,
                    "数据迁移没有完成，仍在使用原数据目录。",
                  )
                : status.migrationNotice.message,
            tone:
              status.migrationNotice.status === "failed" ? "danger" : "success",
          });
        } else if (!status.available) {
          setStorageMessage({
            text: "当前数据目录不可用。请重新连接对应磁盘，然后重启 Mine Mail。",
            tone: "danger",
          });
        }
      })
      .catch((error) => {
        if (!active) return;
        setStorageState("error");
        setStorageMessage({
          text: errorMessage(error, "无法读取本地存储信息。"),
          tone: "danger",
        });
      });
    return () => {
      active = false;
    };
  }, [activeSection, storageClient]);

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
    setAuthorizationGuideProvider(null);
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
    setAuthorizationGuideProvider(null);
  }, [accountFlow, accountSubmitStatus]);

  useEffect(() => {
    scrollRef.current?.scrollTo?.({ top: 0, behavior: "smooth" });
    setAccountMenu(null);
    setPendingAccountRemoval(null);
    setEditingAccountRemark(null);
    setAccountRemarkError(null);
  }, [accountFlow, activeSection, authorizationGuideProvider, selectedProvider]);

  useEffect(() => {
    if (
      authorizationGuideProvider ||
      !authorizationGuideReturnFocusRef.current
    ) {
      return;
    }
    authorizationGuideReturnFocusRef.current = false;
    authorizationGuideButtonRef.current?.focus();
  }, [authorizationGuideProvider]);

  useEffect(() => {
    if (!accountMenu) return undefined;
    const menu = accountMenuRef.current;
    menu?.querySelector('[role="menuitem"]')?.focus();

    const handlePointerDown = (event) => {
      if (
        menu?.contains(event.target) ||
        accountMenuTriggerRef.current?.contains(event.target)
      ) {
        return;
      }
      setAccountMenu(null);
    };

    document.addEventListener("pointerdown", handlePointerDown);
    return () => document.removeEventListener("pointerdown", handlePointerDown);
  }, [accountMenu]);

  useEffect(() => {
    if (!editingAccountRemark) return;
    accountRemarkInputRef.current?.focus();
    accountRemarkInputRef.current?.select();
  }, [editingAccountRemark]);

  const updateSettings = (updater) => {
    const next = typeof updater === "function" ? updater(value) : updater;
    setValue(next);
    void onSave(next);
  };

  const openAccountOverview = () => {
    setAccountFlow("overview");
    setSelectedProvider(null);
    setRepairingAccount(false);
    setAuthorizationGuideProvider(null);
  };

  const openAddAccount = () => {
    if (accountStatus?.canAddAccount === false) return;
    setActiveSection("account");
    setAccountFlow("providers");
    setSelectedProvider(null);
    setRepairingAccount(false);
    setAuthorizationGuideProvider(null);
  };

  const selectAccountProvider = (provider) => {
    onAccountProviderChange?.(provider);
    setSelectedProvider(provider);
  };

  const closeAuthorizationGuide = () => {
    authorizationGuideReturnFocusRef.current = true;
    setAuthorizationGuideProvider(null);
  };

  const openAccountRemarkEditor = (account, returnFocusTarget) => {
    setAccountMenu(null);
    setPendingAccountRemoval(null);
    if (returnFocusTarget) {
      accountActionReturnFocusRef.current = returnFocusTarget;
    }
    setEditingAccountRemark(account);
    setAccountRemarkValue(account.remark || "");
    setAccountRemarkError(null);
  };

  const handleAccountMenuKeyDown = (event) => {
    const items = Array.from(
      event.currentTarget.querySelectorAll('[role="menuitem"]:not(:disabled)'),
    );
    const currentIndex = items.indexOf(document.activeElement);

    if (event.key === "Escape") {
      event.preventDefault();
      setAccountMenu(null);
      accountMenuTriggerRef.current?.focus();
      return;
    }

    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const nextIndex =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? items.length - 1
          : event.key === "ArrowDown"
            ? (currentIndex + 1) % items.length
            : (currentIndex - 1 + items.length) % items.length;
    items[nextIndex]?.focus();
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
      accountActionReturnFocusRef.current?.focus();
    } catch (error) {
      setAccountRemarkError(
        errorMessage(error, "邮箱备注没有保存，请重试。"),
      );
    } finally {
      setIsAccountRemarkSaving(false);
    }
  };

  const chooseStorageDirectory = async () => {
    if (
      !storageClient.isSupported ||
      !storageStatus?.available ||
      ["loading", "choosing", "migrating"].includes(storageState)
    ) {
      return;
    }
    setStorageState("choosing");
    setStorageMessage(null);
    try {
      const selected = await storageClient.chooseDirectory();
      setStorageState("idle");
      if (selected) setPendingStorageDirectory(selected);
    } catch (error) {
      setStorageState("error");
      setStorageMessage({
        text: errorMessage(error, "无法打开数据目录选择器。"),
        tone: "danger",
      });
    }
  };

  const migrateStorageDirectory = async () => {
    if (!pendingStorageDirectory || storageState === "migrating") return;
    setStorageState("migrating");
    setStorageMessage(null);
    let migrationPrepared = false;
    try {
      await storageClient.prepareMigration(pendingStorageDirectory);
      migrationPrepared = true;
      await storageClient.relaunch();
    } catch (error) {
      let migrationCancelled = true;
      if (migrationPrepared) {
        try {
          await storageClient.cancelMigration();
        } catch {
          migrationCancelled = false;
        }
      }
      setStorageState("error");
      setPendingStorageDirectory(null);
      setStorageMessage({
        text: migrationCancelled
          ? errorMessage(error, "数据迁移没有开始，原目录不会改变。")
          : "应用重启失败，待执行的迁移任务也无法撤销。请先不要重启 Mine Mail，并确认当前系统数据目录可写。",
        tone: "danger",
      });
    }
  };

  const closeStorageDialog = () => {
    if (storageState !== "migrating") setPendingStorageDirectory(null);
  };

  const focusSettingsCloseAfterUpdateMinimize = () => {
    Promise.resolve().then(() => settingsCloseRef.current?.focus());
  };

  const closeUpdateDialog = () => {
    const downloadActive = ["installing", "cancelling"].includes(updateStatus);
    closeUpdateDialogController();
    if (downloadActive) focusSettingsCloseAfterUpdateMinimize();
  };

  const minimizeUpdateDialog = () => {
    minimizeUpdateDialogController();
    focusSettingsCloseAfterUpdateMinimize();
  };

  const closeAccountRemarkEditor = () => {
    if (isAccountRemarkSaving) return;
    setEditingAccountRemark(null);
    setAccountRemarkError(null);
    accountActionReturnFocusRef.current?.focus();
  };

  const storageDialogFocus = useConfirmDialogFocus({
    open: Boolean(pendingStorageDirectory),
    isPending: storageState === "migrating",
    initialFocusRef: storageCancelRef,
    returnFocusRef: storageDialogReturnFocusRef,
    onCancel: closeStorageDialog,
  });
  const updateDialogFocus = useConfirmDialogFocus({
    open: Boolean(availableUpdate && isUpdateDialogOpen),
    isPending: ["installing", "cancelling"].includes(updateStatus),
    allowCancelWhilePending: true,
    initialFocusRef: updateCancelRef,
    returnFocusRef: updateDialogReturnFocusRef,
    onCancel: closeUpdateDialog,
  });
  const mcpEnableDialogFocus = useConfirmDialogFocus({
    open: isMcpEnablePending,
    initialFocusRef: mcpEnableCancelRef,
    onCancel: () => setIsMcpEnablePending(false),
  });
  const mcpHelpDialogFocus = useConfirmDialogFocus({
    open: isMcpHelpOpen,
    initialFocusRef: mcpHelpCloseRef,
    onCancel: () => setIsMcpHelpOpen(false),
  });
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

        <nav
          ref={settingsNavRef}
          className="settings-nav"
          aria-label="设置菜单"
          data-selection-visible={navSelectionVisible || undefined}
          data-selection-motion-ready={navSelectionMotionReady || undefined}
          style={navSelectionStyle}
        >
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
            role={saveStatus === "error" ? undefined : "status"}
            aria-live={saveStatus === "error" ? undefined : "polite"}
            aria-atomic="true"
            aria-hidden={saveStatus === "error" || undefined}
          >
            {saveStateLabel}
          </span>
          <IconButton
            ref={settingsCloseRef}
            className="settings-close"
            label="关闭设置"
            onClick={onClose}
            disabled={storageState === "migrating"}
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
                      const legacyOutlook =
                        connectedAccount.provider === "outlook";
                      const isEditingRemark =
                        editingAccountRemark?.accountId ===
                        connectedAccount.accountId;
                      const remarkInputId = `account-remark-${connectedAccount.accountId}`;
                      const remarkErrorId = `${remarkInputId}-error`;
                      return (
                        <div
                          className="settings-account-card"
                          data-active={active}
                          data-remark-editing={isEditingRemark || undefined}
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
                            {legacyOutlook ? (
                              <small>已缓存邮件可读 · 当前不支持重新连接</small>
                            ) : credentialIssue ? (
                              <CredentialWarning />
                            ) : null}
                          </span>
                          {isEditingRemark ? (
                            <form
                              className="settings-account-remark-editor"
                              aria-label={`${connectedAccount.email} 账户备注`}
                              aria-busy={isAccountRemarkSaving || undefined}
                              onKeyDown={(event) => {
                                if (event.key !== "Escape") return;
                                event.preventDefault();
                                event.stopPropagation();
                                closeAccountRemarkEditor();
                              }}
                              onSubmit={(event) => {
                                event.preventDefault();
                                void saveAccountRemark();
                              }}
                            >
                              <span className="settings-account-remark-editor__label">
                                备注
                              </span>
                              <span
                                id={`${remarkInputId}-fields`}
                                className="settings-account-remark-editor__fields"
                              >
                                <input
                                  ref={accountRemarkInputRef}
                                  id={remarkInputId}
                                  className="settings-account-remark-editor__input"
                                  type="text"
                                  aria-label="账户备注"
                                  aria-describedby={
                                    accountRemarkError ? remarkErrorId : undefined
                                  }
                                  aria-invalid={
                                    accountRemarkError ? "true" : undefined
                                  }
                                  value={accountRemarkValue}
                                  maxLength={40}
                                  autoComplete="off"
                                  disabled={isAccountRemarkSaving}
                                  placeholder="输入备注"
                                  onChange={(event) => {
                                    setAccountRemarkValue(event.target.value);
                                    setAccountRemarkError(null);
                                  }}
                                />
                                <button
                                  className="settings-account-remark-editor__save"
                                  type="submit"
                                  aria-busy={isAccountRemarkSaving || undefined}
                                  disabled={isAccountRemarkSaving}
                                >
                                  {isAccountRemarkSaving ? "保存中…" : "保存"}
                                </button>
                              </span>
                              {accountRemarkError ? (
                                <span
                                  id={remarkErrorId}
                                  className="settings-account-remark-editor__error"
                                  role="alert"
                                  aria-live="assertive"
                                  aria-atomic="true"
                                >
                                  {accountRemarkError}
                                </span>
                              ) : null}
                              <ConfirmDialogStatus>
                                {isAccountRemarkSaving
                                  ? "正在保存邮箱备注…"
                                  : null}
                              </ConfirmDialogStatus>
                            </form>
                          ) : null}
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
                          <span
                            className="settings-account-menu-wrap"
                            onBlur={(event) => {
                              if (!event.currentTarget.contains(event.relatedTarget)) {
                                setAccountMenu(null);
                              }
                            }}
                          >
                            <IconButton
                              className="settings-account-action"
                              label={`管理 ${connectedAccount.email}`}
                              title="更多账户操作"
                              aria-haspopup="menu"
                              aria-expanded={accountMenu === connectedAccount.accountId}
                              aria-controls={
                                accountMenu === connectedAccount.accountId
                                  ? `settings-account-menu-${connectedAccount.accountId}`
                                  : undefined
                              }
                              onClick={(event) => {
                                accountActionReturnFocusRef.current =
                                  event.currentTarget;
                                accountMenuTriggerRef.current = event.currentTarget;
                                setAccountMenu((current) =>
                                  current === connectedAccount.accountId
                                    ? null
                                    : connectedAccount.accountId,
                                );
                              }}
                            >
                              <DotsThree size={20} weight="bold" />
                            </IconButton>
                            {accountMenu === connectedAccount.accountId ? (
                              <span
                                ref={accountMenuRef}
                                id={`settings-account-menu-${connectedAccount.accountId}`}
                                className="settings-account-menu"
                                role="menu"
                                onKeyDown={handleAccountMenuKeyDown}
                              >
                                <button
                                  type="button"
                                  role="menuitem"
                                  tabIndex={-1}
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
                                  tabIndex={-1}
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
                    onClick={() => selectAccountProvider(provider.id)}
                  >
                    <ProviderMark provider={provider.id} />
                    <span>
                      <strong>{provider.label}</strong>
                      <small>{provider.description}</small>
                    </span>
                    <CaretRight size={17} aria-hidden="true" />
                  </button>
                ))}
              </div>
            </section>
          ) : null}

          {activeSection === "account" &&
          accountFlow === "providers" &&
          selectedProvider ? (
            <>
              <section
                className="settings-page settings-page--flow"
                aria-labelledby="connect-title"
                hidden={Boolean(authorizationGuideProvider)}
              >
                <header className="settings-flow-heading">
                  <IconButton
                    label={
                      repairingAccount ? "返回账户设置" : "返回选择邮箱服务商"
                    }
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
                      {selectedProvider === "outlook"
                        ? "历史账户"
                        : repairingAccount
                          ? "修复账户"
                          : "添加账户"}
                    </p>
                    <h3 id="connect-title">
                      {selectedProvider === "outlook"
                        ? "Outlook 账户仅可读取缓存"
                        : `连接${providerNames[selectedProvider] || "邮箱"}`}
                    </h3>
                    <p>
                      {selectedProvider === "outlook"
                        ? legacyOutlookNotice
                        : providerDescriptions[selectedProvider]}
                    </p>
                  </span>
                </header>

                <div className="settings-account-setup">
                  {selectedProvider === "outlook" ? (
                    <div className="settings-account-empty" role="status">
                      <ProviderMark provider="outlook" />
                      <span>
                        <strong>此账户暂时无法重新连接</strong>
                        <small>
                          保留账户即可继续阅读本地缓存；移除账户前，请确认是否还需要这些本地邮件。
                        </small>
                      </span>
                    </div>
                  ) : (
                    <AccountSetupForm
                      key={selectedProvider}
                      presets={accountPresets}
                      status={repairingAccount ? accountStatus : null}
                      submitStatus={accountSubmitStatus}
                      error={
                        accountErrorProvider &&
                        accountErrorProvider !== selectedProvider
                          ? null
                          : accountError
                      }
                      initialProvider={selectedProvider}
                      showProviderPicker={false}
                      onSubmit={onConfigureAccount}
                      onGoogle={onConnectGoogle}
                      authorizationGuideButtonRef={authorizationGuideButtonRef}
                      onOpenAuthorizationGuide={setAuthorizationGuideProvider}
                    />
                  )}
                </div>
              </section>
              {authorizationGuideProvider ? (
                <AuthorizationGuide
                  provider={authorizationGuideProvider}
                  onBack={closeAuthorizationGuide}
                />
              ) : null}
            </>
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
                          aria-expanded={isRemoteImageHelpOpen}
                          aria-controls="remote-image-risk"
                          onClick={() => setIsRemoteImageHelpOpen(true)}
                          onFocus={() => setIsRemoteImageHelpOpen(true)}
                          onBlur={() => setIsRemoteImageHelpOpen(false)}
                          onKeyDown={(event) => {
                            if (event.key !== "Escape") return;
                            event.preventDefault();
                            setIsRemoteImageHelpOpen(false);
                            event.currentTarget.blur();
                          }}
                        >
                          <Question size={13} weight="bold" />
                        </button>
                        <span
                          id="remote-image-risk"
                          className="settings-help__tooltip"
                          data-open={isRemoteImageHelpOpen || undefined}
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

                <div className="settings-mcp-group">
                  <div className="settings-preference-row settings-preference-row--toggle">
                    <span>
                      <span className="settings-preference-row__title">
                        <strong id="settings-mcp-enabled-label">开启 MCP</strong>
                        <button
                          type="button"
                          className="settings-help__button"
                          aria-label="了解 Mine Mail MCP 支持的工具"
                          aria-haspopup="dialog"
                          aria-expanded={isMcpHelpOpen}
                          onClick={(event) => {
                            event.preventDefault();
                            setIsMcpHelpOpen(true);
                          }}
                        >
                          <Question size={13} weight="bold" />
                        </button>
                      </span>
                      <small>Mine Mail 在前台或托盘运行时可用。</small>
                    </span>
                    <input
                      type="checkbox"
                      aria-labelledby="settings-mcp-enabled-label"
                      checked={Boolean(value.mcpEnabled)}
                      onChange={(event) => {
                        if (event.target.checked) {
                          setIsMcpEnablePending(true);
                        } else {
                          updateSettings((current) => ({
                            ...current,
                            mcpEnabled: false,
                          }));
                        }
                      }}
                    />
                  </div>

                  <label className="settings-preference-row settings-preference-row--toggle settings-preference-row--child">
                    <span>
                      <strong>获取信息</strong>
                      <small>允许读取、检索、同步、下载附件和整理邮件。</small>
                    </span>
                    <input
                      type="checkbox"
                      checked={Boolean(value.mcpInformationEnabled)}
                      disabled={!value.mcpEnabled}
                      onChange={(event) =>
                        updateSettings((current) => ({
                          ...current,
                          mcpInformationEnabled: event.target.checked,
                        }))
                      }
                    />
                  </label>

                  <label className="settings-preference-row settings-preference-row--toggle settings-preference-row--child">
                    <span>
                      <strong>发送邮件</strong>
                      <small>允许管理草稿、添加附件、回复、转发和真正发送邮件。</small>
                    </span>
                    <input
                      type="checkbox"
                      checked={Boolean(value.mcpSendEnabled)}
                      disabled={!value.mcpEnabled}
                      onChange={(event) =>
                        updateSettings((current) => ({
                          ...current,
                          mcpSendEnabled: event.target.checked,
                        }))
                      }
                    />
                  </label>
                </div>
              </div>
            </section>
          ) : null}

          {activeSection === "agent" ? (
            <AgentSettings client={agentClient} />
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
                  <strong>{displayAppVersion(appVersion)}</strong>
                  <span>当前安装版本</span>
                </span>
                <nav
                  className="settings-version-links"
                  aria-label="隐私、条款与数据说明"
                >
                  {productLinks.map((link) => (
                    <a
                      className="settings-version-link"
                      href={link.url}
                      key={link.url}
                      onClick={(event) => {
                        event.preventDefault();
                        onOpenExternalLink(link.url);
                      }}
                    >
                      {link.label}
                    </a>
                  ))}
                </nav>
                <button
                  ref={updateDialogReturnFocusRef}
                  type="button"
                  className="secondary-button"
                  onClick={() => void checkForUpdate()}
                  disabled={
                    !updateClient.isSupported ||
                    ["checking", "installing", "cancelling"].includes(
                      updateStatus,
                    )
                  }
                >
                  {updateStatus === "checking"
                    ? "正在检查…"
                    : ["installing", "cancelling"].includes(updateStatus)
                      ? "正在更新…"
                      : updateStatus === "available" && availableUpdate
                        ? "查看更新"
                      : "检查更新"}
                </button>
              </div>
              <p
                className="settings-version-note"
                data-tone={updateStatus === "error" ? "danger" : undefined}
                role={updateStatus === "error" ? "alert" : undefined}
                aria-live={updateStatus === "error" ? "assertive" : "polite"}
                aria-atomic="true"
              >
                {updateMessage || (
                  <>
                    项目在{" "}
                    <a
                      className="settings-version-note__link"
                      href={sourceRepositoryUrl}
                      onClick={(event) => {
                        event.preventDefault();
                        onOpenExternalLink(sourceRepositoryUrl);
                      }}
                    >
                      Tantless/mine-mail
                    </a>
                    {" "}上开源，如有问题与反馈，可以在 GitHub Issues 中提出。
                    {!updateClient.isSupported
                      ? " 浏览器预览不执行更新，请使用 Mine Mail 桌面应用。"
                      : null}
                  </>
                )}
              </p>

              <section
                className="settings-storage-section"
                aria-labelledby="settings-storage-title"
                aria-busy={storageState === "loading"}
              >
                <header className="settings-storage-section__heading">
                  <span className="settings-storage-section__icon">
                    <HardDrives size={21} weight="duotone" />
                  </span>
                  <span>
                    <strong id="settings-storage-title">本地数据存储</strong>
                    <small>邮件、用户资源和界面缓存保存在此目录。</small>
                  </span>
                  <strong className="settings-storage-section__total">
                    {storageState === "loading"
                      ? "正在统计…"
                      : formatStorageBytes(storageStatus?.totalBytes)}
                  </strong>
                </header>

                <div className="settings-storage-location">
                  <strong className="settings-storage-location__label">
                    存储位置
                  </strong>
                  <div className="settings-storage-location__capsule">
                    <small className="settings-storage-location__kind">
                      {storageLocationLabel(storageStatus?.locationKind)}
                    </small>
                    <span
                      className="settings-storage-location__divider"
                      aria-hidden="true"
                    />
                    <strong
                      className="settings-storage-location__path"
                      title={storageStatus?.dataPath}
                    >
                      {storageStatus?.dataPath || "正在读取数据目录…"}
                    </strong>
                    <IconButton
                      ref={storageDialogReturnFocusRef}
                      className="settings-storage-change"
                      label="更改位置"
                      title={
                        storageState === "choosing"
                          ? "正在选择存储位置…"
                          : "更改存储位置"
                      }
                      onClick={() => void chooseStorageDirectory()}
                      disabled={
                        !storageClient.isSupported ||
                        !storageStatus?.available ||
                        ["loading", "choosing", "migrating"].includes(
                          storageState,
                        )
                      }
                    >
                      <FolderOpen size={18} weight="bold" />
                    </IconButton>
                  </div>
                </div>

                <div className="settings-storage-usage" aria-label="本地数据空间占用">
                  <div
                    className="settings-storage-composition"
                    role="img"
                    aria-label={storageCompositionLabel(storageStatus?.categories)}
                  >
                    {(storageStatus?.categories || [])
                      .filter((category) => Number(category.bytes) > 0)
                      .map((category) => (
                        <TooltipTarget
                          key={category.id}
                          label={`${category.label} · ${formatStorageBytes(category.bytes)}`}
                        >
                          <span
                            className="settings-storage-composition__segment"
                            data-storage-category={category.id}
                            style={{ flexGrow: Number(category.bytes) }}
                            aria-hidden="true"
                          />
                        </TooltipTarget>
                      ))}
                  </div>
                </div>

                <p
                  className="settings-storage-message"
                  data-tone={storageMessage?.tone}
                  role={storageMessage?.tone === "danger" ? "alert" : undefined}
                  aria-live={
                    storageMessage?.tone === "danger" ? "assertive" : "polite"
                  }
                  aria-atomic="true"
                >
                  {storageMessage?.text ||
                    (storageClient.isSupported
                      ? "更改位置时，Mine Mail 会在重启后校验并迁移原数据。"
                      : "浏览器预览不读取或迁移桌面数据。")}
                </p>
              </section>
            </section>
          ) : null}

          {saveStatus === "error" ? (
            <p
              className="settings-error"
              role="alert"
              aria-live="assertive"
              aria-atomic="true"
            >
              设置没有保存，请重试。
            </p>
          ) : null}
        </div>
      </div>

      {isMcpEnablePending ? (
        <div
          className="confirm-layer"
          onPointerDown={mcpEnableDialogFocus.onBackdropPointerDown}
        >
          <section
            ref={mcpEnableDialogFocus.dialogRef}
            className="confirm-dialog mcp-confirm-dialog"
            role="dialog"
            tabIndex={-1}
            aria-modal="true"
            aria-labelledby="mcp-enable-title"
            aria-describedby="mcp-enable-description"
            onKeyDown={mcpEnableDialogFocus.onDialogKeyDown}
          >
            <header>
              <span className="confirm-dialog__icon" aria-hidden="true">
                <SlidersHorizontal size={22} weight="duotone" />
              </span>
              <IconButton
                label="取消开启 MCP"
                onClick={() => setIsMcpEnablePending(false)}
              >
                <X size={18} />
              </IconButton>
            </header>
            <h2 id="mcp-enable-title">开启 MCP？</h2>
            <p id="mcp-enable-description">
              开启后，本机 Agent 可按所选权限读取或发送邮件。
            </p>
            <footer>
              <button
                ref={mcpEnableCancelRef}
                type="button"
                className="secondary-button"
                onClick={() => setIsMcpEnablePending(false)}
              >
                取消
              </button>
              <button
                type="button"
                className="send-button"
                onClick={() => {
                  setIsMcpEnablePending(false);
                  updateSettings((current) => ({
                    ...current,
                    mcpEnabled: true,
                  }));
                }}
              >
                确认开启
              </button>
            </footer>
          </section>
        </div>
      ) : null}

      {isMcpHelpOpen ? (
        <div
          className="confirm-layer"
          onPointerDown={mcpHelpDialogFocus.onBackdropPointerDown}
        >
          <section
            ref={mcpHelpDialogFocus.dialogRef}
            className="confirm-dialog mcp-help-dialog"
            role="dialog"
            tabIndex={-1}
            aria-modal="true"
            aria-labelledby="mcp-help-title"
            aria-describedby="mcp-help-description"
            onKeyDown={mcpHelpDialogFocus.onDialogKeyDown}
          >
            <header>
              <span className="confirm-dialog__icon" aria-hidden="true">
                <Question size={22} weight="duotone" />
              </span>
              <IconButton
                label="关闭 MCP 工具说明"
                onClick={() => setIsMcpHelpOpen(false)}
              >
                <X size={18} />
              </IconButton>
            </header>
            <h2 id="mcp-help-title">Mine Mail MCP</h2>
            <p id="mcp-help-description">供本机 AI 助理安全调用邮件能力。</p>
            <dl className="mcp-help-dialog__tools vertical-scroll-surface">
              {mcpToolGroups.map((group) => (
                <div key={group.label}>
                  <dt>{group.label}</dt>
                  <dd>
                    {group.tools.map(([name, description]) => (
                      <span key={name}>
                        <code>{name}</code>
                        {description}
                      </span>
                    ))}
                  </dd>
                </div>
              ))}
            </dl>
            <footer>
              <button
                ref={mcpHelpCloseRef}
                type="button"
                className="send-button"
                onClick={() => setIsMcpHelpOpen(false)}
              >
                知道了
              </button>
            </footer>
          </section>
        </div>
      ) : null}

      {pendingStorageDirectory ? (
        <div
          className="confirm-layer"
          data-pending={storageState === "migrating" || undefined}
          onPointerDown={storageDialogFocus.onBackdropPointerDown}
        >
          <section
            ref={storageDialogFocus.dialogRef}
            className="confirm-dialog storage-migration-dialog"
            role="dialog"
            tabIndex={-1}
            aria-modal="true"
            aria-busy={storageState === "migrating" || undefined}
            aria-labelledby="storage-migration-title"
            aria-describedby="storage-migration-description"
            onKeyDown={storageDialogFocus.onDialogKeyDown}
          >
            <header>
              <span className="confirm-dialog__icon" aria-hidden="true">
                <HardDrives size={22} weight="duotone" />
              </span>
              <IconButton
                label="取消数据迁移"
                onClick={closeStorageDialog}
                disabled={storageState === "migrating"}
              >
                <X size={18} />
              </IconButton>
            </header>
            <h2 id="storage-migration-title">迁移本地数据</h2>
            <p id="storage-migration-description">
              Mine Mail 将在重启后把约{" "}
              {formatStorageBytes(storageStatus?.totalBytes)} 数据迁移到新目录。
              新副本校验完成前，原数据不会删除。
            </p>
            <div className="storage-migration-dialog__path">
              <small>新数据目录</small>
              <strong title={pendingStorageDirectory}>
                {pendingStorageDirectory}
              </strong>
            </div>
            <p className="storage-migration-dialog__note">
              迁移期间请保持目标磁盘连接，不要移动或删除原数据目录。
            </p>
            <footer>
              <button
                ref={storageCancelRef}
                type="button"
                className="secondary-button"
                onClick={closeStorageDialog}
                disabled={storageState === "migrating"}
              >
                取消
              </button>
              <button
                type="button"
                className="send-button"
                onClick={() => void migrateStorageDirectory()}
                disabled={storageState === "migrating"}
              >
                <HardDrives size={17} weight="bold" />
                {storageState === "migrating" ? "正在准备…" : "迁移并重启"}
              </button>
            </footer>
            <ConfirmDialogStatus>
              {storageState === "migrating"
                ? "正在准备数据迁移并重启…"
                : null}
            </ConfirmDialogStatus>
          </section>
        </div>
      ) : null}

      {availableUpdate && isUpdateDialogOpen ? (
        <div
          className="confirm-layer"
          onPointerDown={updateDialogFocus.onBackdropPointerDown}
        >
          <section
            ref={updateDialogFocus.dialogRef}
            className="confirm-dialog update-confirm-dialog"
            role="dialog"
            tabIndex={-1}
            aria-modal="true"
            aria-busy={updateStatus === "installing" || undefined}
            aria-labelledby="update-confirm-title"
            aria-describedby="update-confirm-description"
            onKeyDown={updateDialogFocus.onDialogKeyDown}
          >
            <header>
              <span className="confirm-dialog__icon" aria-hidden="true">
                <DownloadSimple size={22} weight="duotone" />
              </span>
              <span className="update-confirm-dialog__actions">
                {["installing", "cancelling"].includes(updateStatus) ? (
                  <IconButton
                    className="update-confirm-dialog__cancel-download"
                    label={
                      updateStatus === "cancelling"
                        ? "正在取消更新下载"
                        : updateProgress?.stage === "installing"
                          ? "更新已下载，无法取消"
                          : "取消更新下载"
                    }
                    onClick={() => void cancelDownload()}
                    disabled={
                      !isDownloadCancellable || updateStatus === "cancelling"
                    }
                  >
                    <StopCircle size={18} weight="regular" />
                  </IconButton>
                ) : null}
                <IconButton
                  label={
                    ["installing", "cancelling"].includes(updateStatus)
                      ? "收起下载进度"
                      : "暂不更新"
                  }
                  onClick={closeUpdateDialog}
                >
                  <X size={18} />
                </IconButton>
              </span>
            </header>
            <h2 id="update-confirm-title">
              发现 Mine Mail {displayAppVersion(availableUpdate.version)}
            </h2>
            <p id="update-confirm-description">
              当前为 {displayAppVersion(appVersion)}。是否下载并安装来自 GitHub
              Release 的签名更新？
            </p>
            {availableUpdate.notes ? (
              <div className="update-confirm-dialog__notes vertical-scroll-surface">
                <small>更新说明</small>
                <p>{availableUpdate.notes}</p>
              </div>
            ) : null}
            {["installing", "cancelling"].includes(updateStatus) ? (
              <div
                className="update-confirm-dialog__progress"
                role="status"
                aria-live="polite"
                aria-atomic="true"
              >
                <span>
                  {updateProgress?.stage === "installing"
                    ? "正在启动安装程序…"
                    : updateStatus === "cancelling"
                      ? "正在取消下载…"
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
              <p
                className="settings-error"
                role="alert"
                aria-live="assertive"
                aria-atomic="true"
              >
                {updateMessage}
              </p>
            ) : null}
            {["installing", "cancelling"].includes(updateStatus) ? (
              <footer>
                <button
                  type="button"
                  className="secondary-button"
                  onClick={minimizeUpdateDialog}
                >
                  收起到后台
                </button>
              </footer>
            ) : (
              <footer>
                <button
                  ref={updateCancelRef}
                  type="button"
                  className="secondary-button"
                  onClick={closeUpdateDialog}
                >
                  暂不更新
                </button>
                <button
                  type="button"
                  className="send-button"
                  onClick={() => void installAvailableUpdate()}
                >
                  <DownloadSimple size={17} weight="bold" />
                  下载并安装
                </button>
              </footer>
            )}
            <ConfirmDialogStatus>
              {["installing", "cancelling"].includes(updateStatus)
                ? "正在下载并安装更新…"
                : null}
            </ConfirmDialogStatus>
          </section>
        </div>
      ) : null}

      <AccountRemovalDialog
        account={pendingAccountRemoval}
        isRemoving={accountSubmitStatus === "saving"}
        returnFocusRef={accountActionReturnFocusRef}
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
