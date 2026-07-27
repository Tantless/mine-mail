import { PencilSimple, X } from "@phosphor-icons/react";
import { initials } from "../utils/formatters.js";
import { brandRules } from "./brandAvatars.js";
import { TooltipTarget } from "./Tooltip.jsx";

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
  if (brand.originalMark) {
    return (
      <svg
        className="brand-mark__icon brand-mark__icon--original"
        viewBox={brand.originalMark.viewBox}
        aria-hidden="true"
      >
        {brand.originalMark.paths.map((path, index) => (
          <path key={`${brand.id}-${index}`} fill={path.fill} d={path.d} />
        ))}
      </svg>
    );
  }

  if (brand.simpleIcon) {
    return (
      <svg
        className="brand-mark__icon"
        viewBox="0 0 24 24"
        aria-hidden="true"
      >
        <path fill="currentColor" d={brand.simpleIcon.path} />
      </svg>
    );
  }

  if (brand.Icon) {
    const Icon = brand.Icon;
    return (
      <Icon
        className="brand-mark__icon"
        weight={brand.iconWeight}
        aria-hidden="true"
      />
    );
  }

  if (brand.mark === "microsoft") {
    return (
      <span className="brand-mark__microsoft" aria-hidden="true">
        <i /><i /><i /><i />
      </span>
    );
  }

  return (
    <span className="brand-mark__letters" aria-hidden="true">
      {brand.letters}
    </span>
  );
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
  const brandStyle = brand
    ? {
        "--brand-avatar-background": brand.background,
        "--brand-avatar-foreground": brand.foreground,
      }
    : undefined;

  return (
    <span
      className={classes}
      style={brandStyle}
      aria-label={
        customSrc ? `${label} 的自定义头像` : brand?.label || undefined
      }
    >
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
      <TooltipTarget label={`设置 ${label || email} 的头像`}>
        <label className="avatar-picker__choose">
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
      </TooltipTarget>
      {customSrc ? (
        <TooltipTarget label="恢复默认头像">
          <button
            type="button"
            className="avatar-picker__remove"
            aria-label={`移除 ${label || email} 的自定义头像`}
            onClick={onRemove}
          >
            <X size={10} weight="bold" />
          </button>
        </TooltipTarget>
      ) : null}
    </span>
  );
}
