import { PencilSimple, X } from "@phosphor-icons/react";
import { initials } from "../utils/formatters.js";

const brandRules = [
  { id: "github", label: "GitHub", domains: ["github.com"] },
  { id: "google", label: "Google", domains: ["google.com", "gmail.com"] },
  {
    id: "netease",
    label: "网易邮箱",
    domains: ["163.com", "126.com", "yeah.net", "netease.com"],
  },
  {
    id: "microsoft",
    label: "Microsoft",
    domains: ["microsoft.com", "outlook.com", "live.com", "windows.com"],
  },
  { id: "nintendo", label: "Nintendo", domains: ["nintendo.com", "nintendo.net"] },
  { id: "playstation", label: "PlayStation", domains: ["playstation.com", "sony.com"] },
];

export function normalizeAvatarEmail(value = "") {
  return (value ?? "").trim().toLowerCase();
}

export function avatarToneForEmail(value = "") {
  const normalized = normalizeAvatarEmail(value);
  let hash = 2166136261;
  for (let index = 0; index < normalized.length; index += 1) {
    hash ^= normalized.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0) % 4;
}

function emailDomain(email) {
  const normalized = normalizeAvatarEmail(email);
  const separator = normalized.lastIndexOf("@");
  return separator >= 0 ? normalized.slice(separator + 1) : "";
}

function domainMatches(domain, expected) {
  return domain === expected || domain.endsWith(`.${expected}`);
}

export function trustedBrandForEmail(email) {
  const domain = emailDomain(email);
  if (!domain) return null;
  return (
    brandRules.find((brand) =>
      brand.domains.some((expected) => domainMatches(domain, expected)),
    ) || null
  );
}

function BrandMark({ brand }) {
  if (brand.id === "github") {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M12 .8a11.4 11.4 0 0 0-3.6 22.2c.6.1.8-.2.8-.5v-2.2c-3.3.7-4-1.4-4-1.4-.5-1.4-1.3-1.8-1.3-1.8-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1.1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.8-1.6-2.7-.3-5.5-1.3-5.5-5.7 0-1.3.5-2.3 1.2-3.1-.1-.3-.5-1.5.1-3.1 0 0 1-.3 3.2 1.2a11 11 0 0 1 5.8 0c2.2-1.5 3.2-1.2 3.2-1.2.6 1.6.2 2.8.1 3.1.8.8 1.2 1.8 1.2 3.1 0 4.4-2.8 5.4-5.5 5.7.4.4.8 1.1.8 2.2v3.3c0 .3.2.6.8.5A11.4 11.4 0 0 0 12 .8Z"
        />
      </svg>
    );
  }
  if (brand.id === "microsoft") {
    return (
      <span className="brand-mark__microsoft" aria-hidden="true">
        <i /><i /><i /><i />
      </span>
    );
  }
  return <span className="brand-mark__letters">{{ google: "G", netease: "易", nintendo: "N", playstation: "PS" }[brand.id]}</span>;
}

export function ProfileAvatar({ email, label, customSrc, className = "" }) {
  const brand = customSrc ? null : trustedBrandForEmail(email);
  const tone = customSrc || brand ? null : avatarToneForEmail(email || label);
  const classes = [
    "profile-avatar",
    customSrc ? "profile-avatar--custom" : brand ? "profile-avatar--brand" : "profile-avatar--initials",
    brand ? `profile-avatar--${brand.id}` : "",
    tone == null ? "" : `profile-avatar--tone-${tone}`,
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <span className={classes} aria-label={customSrc ? `${label} 的自定义头像` : brand?.label || undefined}>
      {customSrc ? (
        <img src={customSrc} alt="" />
      ) : brand ? (
        <BrandMark brand={brand} />
      ) : (
        initials(label || email || "?")
      )}
    </span>
  );
}

export function EditableProfileAvatar({
  email,
  label,
  customSrc,
  className = "",
  avatarClassName = "",
  onSelectFile,
  onRemove,
}) {
  return (
    <span className={`avatar-picker ${className}`.trim()}>
      <label className="avatar-picker__choose" title={`设置 ${label || email} 的头像`}>
        <ProfileAvatar
          email={email}
          label={label}
          customSrc={customSrc}
          className={avatarClassName}
        />
        <span className="avatar-picker__edit" aria-hidden="true">
          <PencilSimple size={10} weight="bold" />
        </span>
        <input
          type="file"
          accept="image/png,image/jpeg,image/webp"
          aria-label={`设置 ${label || email} 的头像`}
          onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) void onSelectFile(file);
            event.target.value = "";
          }}
        />
      </label>
      {customSrc ? (
        <button
          type="button"
          className="avatar-picker__remove"
          aria-label={`移除 ${label || email} 的自定义头像`}
          title="恢复默认头像"
          onClick={onRemove}
        >
          <X size={10} weight="bold" />
        </button>
      ) : null}
    </span>
  );
}
