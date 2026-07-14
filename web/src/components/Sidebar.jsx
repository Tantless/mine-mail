import {
  Archive,
  EnvelopeSimple,
  FileText,
  GearSix,
  Tray,
  Palette,
  PaperPlaneTilt,
  PencilSimple,
  Star,
  Trash,
} from "@phosphor-icons/react";

const folders = [
  { id: "inbox", label: "收件箱", icon: Tray, count: 4 },
  { id: "starred", label: "已加星标", icon: Star },
  { id: "sent", label: "已发送", icon: PaperPlaneTilt },
  { id: "drafts", label: "草稿", icon: FileText, count: 1 },
  { id: "archive", label: "归档", icon: Archive },
  { id: "trash", label: "垃圾箱", icon: Trash },
];

const themeOptions = [
  { id: "daylight", label: "日间", swatch: "theme-swatch--daylight" },
  { id: "night", label: "夜间", swatch: "theme-swatch--night" },
  { id: "dusk", label: "黄昏", swatch: "theme-swatch--dusk" },
  { id: "forest", label: "森林", swatch: "theme-swatch--forest" },
];

export function Sidebar({
  activeFolder,
  onFolderChange,
  onCompose,
  theme,
  onThemeChange,
  isThemeMenuOpen,
  onThemeMenuToggle,
}) {
  return (
    <aside className="sidebar" aria-label="邮箱导航">
      <div className="sidebar__scrim" aria-hidden="true" />
      <div className="sidebar__content">
        <div className="brand" aria-label="Mine Mail">
          <span className="brand__mark">
            <EnvelopeSimple size={22} weight="duotone" />
          </span>
          <span className="brand__name">Mine Mail</span>
        </div>

        <button className="compose-button" type="button" onClick={onCompose}>
          <PencilSimple size={19} weight="bold" />
          <span>写信</span>
          <kbd>N</kbd>
        </button>

        <nav className="folder-nav">
          {folders.map((folder) => {
            const FolderIcon = folder.icon;
            const selected = folder.id === activeFolder;
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
                {folder.count ? <span className="folder-nav__count">{folder.count}</span> : null}
              </button>
            );
          })}
        </nav>

        <div className="sidebar__spacer" />

        <div className="account-card">
          <span className="account-card__avatar">M</span>
          <span className="account-card__copy">
            <strong>163 邮箱</strong>
            <small>已连接</small>
          </span>
          <span className="account-card__status" aria-label="已连接" />
        </div>

        <div className="theme-control">
          {isThemeMenuOpen ? (
            <div className="theme-menu" role="menu" aria-label="选择主题">
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
            onClick={onThemeMenuToggle}
            aria-expanded={isThemeMenuOpen}
          >
            <Palette size={19} />
            <span>主题外观</span>
          </button>
          <button type="button" className="sidebar-action">
            <GearSix size={19} />
            <span>设置</span>
          </button>
        </div>
      </div>
    </aside>
  );
}
