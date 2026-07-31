import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
} from "react";
import {
  AddressBook,
  Archive,
  FileText,
  GearSix,
  Plus,
  Tray,
  Palette,
  PaperPlaneTilt,
  PencilSimple,
  SpinnerGap,
  Star,
  Trash,
} from "@phosphor-icons/react";
import { BrandLogo } from "./BrandLogo.jsx";
import { CredentialWarning } from "./CredentialWarning.jsx";
import { ProfileAvatar } from "./ProfileAvatar.jsx";
import { useSlidingSelection } from "../hooks/useSlidingSelection.js";

const folders = [
  { id: "inbox", label: "收件箱", icon: Tray },
  { id: "starred", label: "已收藏", icon: Star },
  { id: "contacts", label: "通讯录", icon: AddressBook },
  { id: "sent", label: "已发送", icon: PaperPlaneTilt },
  { id: "drafts", label: "草稿", icon: FileText },
  { id: "outbox", label: "发件队列", icon: PaperPlaneTilt },
  { id: "archive", label: "归档", icon: Archive },
  { id: "trash", label: "垃圾箱", icon: Trash },
];

const foldersWithCounts = new Set(["inbox"]);
const capabilityFolderRoles = new Set(["archive", "trash"]);
const drawerFocusableSelector = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

const capabilityCopy = {
  archive: {
    mailbox: "归档文件夹",
  },
  trash: {
    mailbox: "垃圾箱",
  },
};

function mailboxCapabilityFor(mailboxCapabilities, role) {
  if (!capabilityFolderRoles.has(role)) return null;
  if (mailboxCapabilities == null) {
    return { role, status: "discovery_pending", retryable: false };
  }
  if (Array.isArray(mailboxCapabilities)) {
    return (
      mailboxCapabilities.find((capability) => capability?.role === role) || {
        role,
        status: "discovery_pending",
        retryable: false,
      }
    );
  }
  return (
    mailboxCapabilities[role] || {
      role,
      status: "discovery_pending",
      retryable: false,
    }
  );
}

function capabilityStateFor(role, capability) {
  const copy = capabilityCopy[role];
  if (!copy || !capability || capability.status === "available") return null;

  if (capability.status === "needs_creation_confirmation") {
    if (role === "archive") return null;
    return {
      actionLabel: `设置${copy.mailbox}`,
      action: "setup",
    };
  }

  if (capability.status === "discovery_pending") {
    return {
      actionLabel: `重新确认${copy.mailbox}`,
      action: capability.retryable ? "retry" : null,
    };
  }

  return {
    actionLabel: `重新确认${copy.mailbox}`,
    action: capability.retryable ? "retry" : null,
  };
}

const themeOptions = [
  { id: "daylight", label: "日间", swatch: "theme-swatch--daylight" },
  { id: "night", label: "夜间", swatch: "theme-swatch--night" },
  { id: "dusk", label: "黄昏", swatch: "theme-swatch--dusk" },
  { id: "forest", label: "森林", swatch: "theme-swatch--forest" },
];

const providerNames = {
  "163": "163 邮箱",
  gmail: "Gmail",
  outlook: "Outlook",
  custom: "自定义邮箱",
};

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

export function Sidebar({
  activeFolder,
  onFolderChange,
  onCompose,
  theme,
  onThemeChange,
  isThemeMenuOpen,
  onThemeMenuToggle,
  onThemeMenuClose,
  counts = {},
  outboxActive = false,
  sentHasNew = false,
  accountStatus,
  isSettingsOpen = false,
  isMailListExpanded = true,
  isFolderSelectionVisible = true,
  mailListControlsId = "mail-list-panel",
  accountAvatarFor,
  onAccountSwitch,
  onAddAccount,
  onOpenSettings,
  mailboxCapabilities = null,
  onMailboxSetup = null,
  onMailboxCapabilityRetry = null,
  isDrawerOpen = false,
  onDrawerClose = null,
  drawerTriggerRef = null,
}) {
  const sidebarRef = useRef(null);
  const folderNavRef = useRef(null);
  const accountSwitcherRef = useRef(null);
  const drawerPreviousFocusRef = useRef(null);
  const themeToggleRef = useRef(null);
  const themeOptionRefs = useRef([]);
  const themeWasOpenRef = useRef(false);
  const themeMenuId = `theme-menu-${useId().replaceAll(":", "")}`;
  const themeMenuOpenRef = useRef(isThemeMenuOpen);
  themeMenuOpenRef.current = isThemeMenuOpen;
  const accounts = connectedAccounts(accountStatus);
  const maxAccounts = Math.max(accountStatus?.maxAccounts || 3, accounts.length);
  const emptySlots = Math.max(0, maxAccounts - accounts.length);
  const hasAvailableAccountSlot = emptySlots > 0;
  const activeAccountId = accountStatus?.activeAccountId || accountStatus?.accountId;
  const folderSelectionKey =
    isSettingsOpen || !isFolderSelectionVisible ? null : activeFolder;
  const {
    motionReady: folderSelectionMotionReady,
    selectionStyle: folderSelectionStyle,
    selectionVisible: folderSelectionVisible,
  } = useSlidingSelection({
    containerRef: folderNavRef,
    layoutKey: folderSelectionKey,
    selectedKey: folderSelectionKey,
  });
  const accountLayoutKey = accounts
    .map((account) => account.accountId)
    .join("|");
  const {
    motionReady: accountSelectionMotionReady,
    selectionStyle: accountSelectionStyle,
    selectionVisible: accountSelectionVisible,
  } = useSlidingSelection({
    containerRef: accountSwitcherRef,
    layoutKey: accountLayoutKey,
    selectedKey: activeAccountId || null,
    selectedSelector: '.account-card[data-active="true"]',
  });

  useLayoutEffect(() => {
    const wasOpen = themeWasOpenRef.current;
    themeWasOpenRef.current = isThemeMenuOpen;
    if (isThemeMenuOpen) {
      const selectedIndex = Math.max(
        0,
        themeOptions.findIndex((option) => option.id === theme),
      );
      themeOptionRefs.current[selectedIndex]?.focus();
    } else if (wasOpen && themeToggleRef.current?.isConnected) {
      themeToggleRef.current.focus();
    }
  }, [isThemeMenuOpen, theme]);

  useEffect(() => {
    if (!isThemeMenuOpen) return undefined;
    const closeOnEscape = (event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onThemeMenuClose?.();
    };

    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [isThemeMenuOpen, onThemeMenuClose]);

  const navigateThemeMenu = (event) => {
    const keys = ["ArrowDown", "ArrowRight", "ArrowUp", "ArrowLeft", "Home", "End"];
    if (!keys.includes(event.key)) return;
    event.preventDefault();
    const options = themeOptionRefs.current.filter(Boolean);
    if (!options.length) return;
    const currentIndex = Math.max(0, options.indexOf(document.activeElement));
    const nextIndex =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? options.length - 1
          : ["ArrowDown", "ArrowRight"].includes(event.key)
            ? (currentIndex + 1) % options.length
            : (currentIndex - 1 + options.length) % options.length;
    options[nextIndex]?.focus();
  };

  useEffect(() => {
    if (!isDrawerOpen) return undefined;
    drawerPreviousFocusRef.current =
      drawerTriggerRef?.current ||
      (document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null);
    const focusable = sidebarRef.current?.querySelector(
      drawerFocusableSelector,
    );
    focusable?.focus();
    const closeOnEscape = (event) => {
      if (event.key !== "Escape" || themeMenuOpenRef.current) return;
      event.preventDefault();
      event.stopPropagation();
      onDrawerClose?.();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("keydown", closeOnEscape);
      const previousFocus = drawerPreviousFocusRef.current;
      drawerPreviousFocusRef.current = null;
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  }, [drawerTriggerRef, isDrawerOpen, onDrawerClose]);

  const trapDrawerFocus = (event) => {
    if (event.key !== "Tab" || !isDrawerOpen || event.defaultPrevented) return;
    const sidebar = sidebarRef.current;
    if (!sidebar) return;
    const focusable = Array.from(
      sidebar.querySelectorAll(drawerFocusableSelector),
    ).filter(
      (element) =>
        element instanceof HTMLElement &&
        element.getAttribute("aria-hidden") !== "true" &&
        element.tabIndex >= 0,
    );
    const first = focusable[0];
    const last = focusable.at(-1);
    const active = document.activeElement;
    if (!first || !last) {
      event.preventDefault();
      sidebar.focus();
      return;
    }
    if (event.shiftKey && (active === first || !sidebar.contains(active))) {
      event.preventDefault();
      last.focus();
      return;
    }
    if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <aside
      ref={sidebarRef}
      className="sidebar"
      aria-label="邮箱导航"
      data-drawer-open={isDrawerOpen || undefined}
      tabIndex={-1}
      onKeyDown={trapDrawerFocus}
    >
      <div className="sidebar__scrim" aria-hidden="true" />
      <div className="sidebar__content">
        <div className="brand" aria-label="Mine Mail">
          <span className="brand__mark" aria-hidden="true">
            <BrandLogo />
          </span>
          <span className="brand__name">Mine Mail</span>
        </div>

        <button className="compose-button" type="button" onClick={onCompose}>
          <PencilSimple size={19} weight="bold" />
          <span>写信</span>
        </button>

        <div className="sidebar__primary vertical-scroll-surface">
          <nav
            ref={folderNavRef}
            className="folder-nav"
            aria-label="邮箱文件夹"
          >
            <span
              className="folder-nav__selection"
              aria-hidden="true"
              data-visible={folderSelectionVisible || undefined}
              data-motion-ready={folderSelectionMotionReady || undefined}
              style={folderSelectionStyle}
            />
            {folders.map((folder) => {
              const FolderIcon = folder.icon;
              const selected = folder.id === folderSelectionKey;
              const controlsMailList =
                !isSettingsOpen && folder.id === activeFolder;
              const capability = mailboxCapabilityFor(
                mailboxCapabilities,
                folder.id,
              );
              const capabilityState = capabilityStateFor(
                folder.id,
                capability,
              );
              const capabilityAction =
                capabilityState?.action === "setup" &&
                typeof onMailboxSetup === "function"
                  ? () => onMailboxSetup(folder.id)
                  : capabilityState?.action === "retry" &&
                      typeof onMailboxCapabilityRetry === "function"
                    ? () => onMailboxCapabilityRetry(folder.id)
                    : null;
              const folderAction =
                !capabilityState && typeof onFolderChange === "function"
                  ? () => onFolderChange(folder.id)
                  : capabilityAction;
              const accessibleLabel = capabilityAction
                ? capabilityState.actionLabel
                : folder.id === "outbox" && outboxActive
                  ? `${folder.label}，有邮件正在队列中处理`
                  : folder.id === "sent" && sentHasNew
                    ? `${folder.label}，有新增邮件`
                    : folder.label;
              return (
                <button
                  key={folder.id}
                  type="button"
                  className="folder-nav__item"
                  data-selected={selected}
                  onClick={folderAction || undefined}
                  disabled={!folderAction}
                  aria-label={accessibleLabel}
                  aria-current={selected ? "page" : undefined}
                  aria-controls={
                    controlsMailList ? mailListControlsId : undefined
                  }
                  aria-expanded={
                    controlsMailList ? isMailListExpanded : undefined
                  }
                >
                  <FolderIcon size={19} weight={selected ? "fill" : "regular"} />
                  <span className="folder-nav__label">{folder.label}</span>
                  {foldersWithCounts.has(folder.id) && counts[folder.id] ? (
                    <span className="folder-nav__count">{counts[folder.id]}</span>
                  ) : null}
                  {folder.id === "outbox" && outboxActive ? (
                    <span
                      className="folder-nav__activity folder-nav__activity--outbox"
                      aria-hidden="true"
                    >
                      <SpinnerGap size={16} weight="bold" />
                    </span>
                  ) : null}
                  {folder.id === "sent" && sentHasNew ? (
                    <span className="folder-nav__new-dot" aria-hidden="true" />
                  ) : null}
                </button>
              );
            })}
          </nav>
        </div>

        <div className="sidebar__footer">
          <div
            ref={accountSwitcherRef}
            className="account-switcher"
            role="group"
            aria-label="已登录邮箱账户"
            data-selection-visible={accountSelectionVisible || undefined}
            data-selection-motion-ready={
              accountSelectionMotionReady || undefined
            }
            style={accountSelectionStyle}
          >
            {hasAvailableAccountSlot ? (
              <button
                type="button"
                className="account-add-slot"
                aria-label={`添加邮箱账户，还可添加 ${emptySlots} 个`}
                onClick={onAddAccount}
              >
                <Plus size={16} weight="bold" aria-hidden="true" />
                <span>添加账户</span>
              </button>
            ) : null}

            {accounts.map((account) => {
              const accountLabel = providerNames[account.provider] || "邮箱账户";
              const displayName = account.remark?.trim() || accountLabel;
              const active = account.accountId === activeAccountId;
              const credentialIssue =
                account.credentialInvalid || account.credentialAvailable === false;
              return (
                <button
                  key={account.accountId}
                  type="button"
                  className="account-card"
                  data-active={active}
                  data-credential-issue={credentialIssue}
                  aria-pressed={active}
                  aria-label={`${active ? "当前账户" : "切换到"} ${
                    account.remark
                      ? `${displayName} ${account.email}`
                      : account.email
                  }${credentialIssue ? "，凭证失效" : ""}`}
                  onClick={() => onAccountSwitch(account.accountId)}
                >
                  <ProfileAvatar
                    className="account-card__avatar"
                    email={account.email}
                    label={displayName}
                    customSrc={accountAvatarFor?.(account.email)}
                  />
                  <span className="account-card__copy">
                    <strong>{displayName}</strong>
                    <small>{account.email}</small>
                  </span>
                  {credentialIssue ? <CredentialWarning compact /> : null}
                </button>
              );
            })}
          </div>

          <div className="theme-control">
            {isThemeMenuOpen ? (
              <div
                className="theme-menu"
                id={themeMenuId}
                role="menu"
                aria-label="选择主题"
                onKeyDown={navigateThemeMenu}
              >
                <p>界面主题</p>
                <div className="theme-menu__grid">
                  {themeOptions.map((option, index) => (
                    <button
                      ref={(element) => {
                        themeOptionRefs.current[index] = element;
                      }}
                      key={option.id}
                      type="button"
                      role="menuitemradio"
                      aria-checked={theme === option.id}
                      className="theme-option"
                      data-selected={theme === option.id}
                      onClick={() => onThemeChange(option.id)}
                    >
                      <span className={`theme-swatch ${option.swatch}`} />
                      <span>{option.label}</span>
                    </button>
                  ))}
                </div>
              </div>
            ) : null}
            <button
              ref={themeToggleRef}
              type="button"
              className="sidebar-action"
              data-theme-menu-toggle="true"
              onClick={onThemeMenuToggle}
              aria-haspopup="menu"
              aria-controls={isThemeMenuOpen ? themeMenuId : undefined}
              aria-expanded={isThemeMenuOpen}
            >
              <Palette size={19} />
              <span>主题外观</span>
            </button>
            <button
              type="button"
              className="sidebar-action"
              data-selected={isSettingsOpen}
              aria-current={isSettingsOpen ? "page" : undefined}
              onClick={onOpenSettings}
            >
              <GearSix size={19} />
              <span>设置</span>
            </button>
          </div>
        </div>
      </div>
    </aside>
  );
}
