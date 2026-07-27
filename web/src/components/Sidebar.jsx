import { useEffect } from "react";
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
  Star,
  Trash,
} from "@phosphor-icons/react";
import { BrandLogo } from "./BrandLogo.jsx";
import { CredentialWarning } from "./CredentialWarning.jsx";
import { ProfileAvatar } from "./ProfileAvatar.jsx";

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

const foldersWithCounts = new Set(["inbox", "outbox"]);

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
  accountStatus,
  isSettingsOpen = false,
  accountAvatarFor,
  onAccountSwitch,
  onAddAccount,
  onOpenSettings,
}) {
  const accounts = connectedAccounts(accountStatus);
  const maxAccounts = Math.max(accountStatus?.maxAccounts || 3, accounts.length);
  const emptySlots = Math.max(0, maxAccounts - accounts.length);
  const hasAvailableAccountSlot = emptySlots > 0;
  const activeAccountId = accountStatus?.activeAccountId || accountStatus?.accountId;

  useEffect(() => {
    if (!isThemeMenuOpen) return undefined;

    const closeOnEscape = (event) => {
      if (event.key === "Escape") onThemeMenuClose?.();
    };

    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [isThemeMenuOpen, onThemeMenuClose]);

  return (
    <aside className="sidebar" aria-label="邮箱导航">
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
          <nav className="folder-nav">
            {folders.map((folder) => {
              const FolderIcon = folder.icon;
              const selected = !isSettingsOpen && folder.id === activeFolder;
              return (
                <button
                  key={folder.id}
                  type="button"
                  className="folder-nav__item"
                  data-selected={selected}
                  onClick={() => onFolderChange(folder.id)}
                  aria-current={selected ? "page" : undefined}
                >
                  <FolderIcon size={19} weight={selected ? "fill" : "regular"} />
                  <span>{folder.label}</span>
                  {foldersWithCounts.has(folder.id) && counts[folder.id] ? (
                    <span className="folder-nav__count">{counts[folder.id]}</span>
                  ) : null}
                </button>
              );
            })}
          </nav>
        </div>

        <div className="sidebar__footer">
          <div className="account-switcher" aria-label="已登录邮箱账户">
            {hasAvailableAccountSlot ? (
              <button
                type="button"
                className="account-add-slot"
                aria-label={`添加邮箱账户，还可添加 ${emptySlots} 个`}
                onClick={onAddAccount}
              >
                <Plus size={16} weight="bold" aria-hidden="true" />
                <span>添加账号</span>
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
                role="menu"
                aria-label="选择主题"
              >
                <p>界面主题</p>
                <div className="theme-menu__grid">
                  {themeOptions.map((option) => (
                    <button
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
              type="button"
              className="sidebar-action"
              data-theme-menu-toggle="true"
              onClick={onThemeMenuToggle}
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
