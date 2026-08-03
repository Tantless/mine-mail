import { useEffect, useId, useMemo, useRef, useState } from "react";
import {
  EnvelopeSimple,
  GoogleLogo,
  Question,
  ShieldCheck,
  ShieldWarning,
} from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { ThemedSelect } from "./ThemedSelect.jsx";

const fallbackPresets = [
  { id: "163", label: "163 邮箱", secret_label: "客户端授权密码" },
  { id: "qq", label: "QQ 邮箱", secret_label: "QQ 邮箱授权码" },
  { id: "gmail", label: "Gmail", oauth: true, secret_label: "Google OAuth" },
  { id: "custom", label: "自定义 IMAP/SMTP", secret_label: "邮箱密码或授权密码" },
];

const smtpSecurityOptions = [
  { value: "implicit_tls", label: "TLS" },
  { value: "start_tls", label: "STARTTLS" },
];

const authorizationGuideProviders = new Set(["163", "qq"]);
const providerEmailDomains = Object.freeze({
  163: "@163.com",
  qq: "@qq.com",
});

function editableEmailValue(value, provider) {
  const domain = providerEmailDomains[provider];
  if (!domain || !value.toLowerCase().endsWith(domain)) return value;
  return value.slice(0, -domain.length);
}

function emailValueForProviderChange(value, currentProvider, nextProvider) {
  const currentDomain = providerEmailDomains[currentProvider];
  const nextDomain = providerEmailDomains[nextProvider];
  if (currentDomain && nextDomain) {
    return editableEmailValue(value, currentProvider);
  }
  if (currentDomain && !nextDomain && value && !value.includes("@")) {
    return `${value}${currentDomain}`;
  }
  return editableEmailValue(value, nextProvider);
}

function normalizedPreset(preset) {
  return {
    id: preset.id ?? preset.provider ?? preset.provider_id,
    label: preset.label ?? preset.name ?? preset.id,
    disabled: Boolean(
      preset.disabled ||
        preset.availableInMvp === false ||
        preset.available_in_mvp === false,
    ),
    note:
      preset.note ??
      preset.authenticationNote ??
      preset.authentication_note ??
      null,
    secretLabel:
      preset.secretLabel ?? preset.secret_label ?? "邮箱密码或客户端授权密码",
    oauth: Boolean(preset.oauth),
  };
}

function resolveProvider(options, requestedProvider) {
  if (options.some((option) => option.id === requestedProvider)) {
    return requestedProvider;
  }
  return options.find((option) => !option.disabled)?.id || options[0]?.id || "163";
}

export function AccountSetupForm({
  presets,
  status,
  submitStatus,
  error,
  onSubmit,
  onGoogle,
  onOpenAuthorizationGuide,
  authorizationGuideButtonRef,
  initialProvider: requestedInitialProvider,
  showProviderPicker = true,
}) {
  const options = useMemo(() => {
    const normalized = (presets?.length ? presets : fallbackPresets)
      .map(normalizedPreset)
      .filter((preset) => preset.id !== "outlook");
    return normalized.length
      ? normalized
      : fallbackPresets.map(normalizedPreset);
  }, [presets]);
  const initialProvider = resolveProvider(
    options,
    requestedInitialProvider || status?.provider,
  );
  const [provider, setProvider] = useState(initialProvider);
  const [email, setEmail] = useState(() =>
    editableEmailValue(status?.email || "", initialProvider),
  );
  const [custom, setCustom] = useState({
    imapHost: "",
    imapPort: 993,
    smtpHost: "",
    smtpPort: 465,
    smtpSecurity: "implicit_tls",
  });
  const [validationError, setValidationError] = useState(null);
  const secretInputId = useId();
  const emailInputId = useId();
  const emailDomainDescriptionId = useId();
  const emailRef = useRef(null);
  const secretRef = useRef(null);
  const imapHostRef = useRef(null);
  const imapPortRef = useRef(null);
  const smtpHostRef = useRef(null);
  const smtpPortRef = useRef(null);

  useEffect(() => {
    if (status?.email) {
      const statusProvider = resolveProvider(
        options,
        requestedInitialProvider || status?.provider,
      );
      setEmail(editableEmailValue(status.email, statusProvider));
    }
    setProvider((current) =>
      resolveProvider(
        options,
        requestedInitialProvider || status?.provider || current,
      ),
    );
  }, [options, requestedInitialProvider, status?.email, status?.provider]);

  const selected = options.find((item) => item.id === provider) || options[0];
  const providerEmailDomain = providerEmailDomains[provider] || null;
  const configurationBlocked = Boolean(selected?.disabled);
  const displayedError = validationError ? validationError.message : error;

  const rejectInvalidField = (field, message, ref) => {
    setValidationError({ field, message });
    ref.current?.focus();
  };

  const handleSubmit = (event) => {
    event.preventDefault();
    if (configurationBlocked || submitStatus === "saving") return;
    if (provider === "gmail") {
      setValidationError(null);
      void onGoogle?.();
      return;
    }

    const emailInput = email.trim();
    const normalizedEmail = providerEmailDomain
      ? `${emailInput}${providerEmailDomain}`
      : emailInput;
    const secret = secretRef.current?.value || "";
    if (!emailInput) {
      rejectInvalidField("email", null, emailRef);
      return;
    }
    const emailIsValid = providerEmailDomain
      ? /^[^\s@]+$/.test(emailInput)
      : /^[^\s@]+@[^\s@]+$/.test(normalizedEmail);
    if (!emailIsValid) {
      rejectInvalidField(
        "email",
        providerEmailDomain ? "邮箱账号格式不正确。" : "邮箱地址格式不正确。",
        emailRef,
      );
      return;
    }
    if (!secret.trim()) {
      rejectInvalidField("secret", null, secretRef);
      return;
    }
    if (provider === "custom") {
      if (!custom.imapHost.trim()) {
        rejectInvalidField("imapHost", null, imapHostRef);
        return;
      }
      const imapPort = Number(custom.imapPort);
      if (!Number.isInteger(imapPort) || imapPort < 1 || imapPort > 65535) {
        rejectInvalidField(
          "imapPort",
          "IMAP 端口应为 1–65535。",
          imapPortRef,
        );
        return;
      }
      if (!custom.smtpHost.trim()) {
        rejectInvalidField("smtpHost", null, smtpHostRef);
        return;
      }
      const smtpPort = Number(custom.smtpPort);
      if (!Number.isInteger(smtpPort) || smtpPort < 1 || smtpPort > 65535) {
        rejectInvalidField(
          "smtpPort",
          "SMTP 端口应为 1–65535。",
          smtpPortRef,
        );
        return;
      }
    }

    setValidationError(null);
    if (secretRef.current) secretRef.current.value = "";
    const request = {
      provider,
      email: normalizedEmail,
      secret,
      ...(provider === "custom"
        ? {
            imap_host: custom.imapHost.trim(),
            imap_port: Number(custom.imapPort),
            smtp_host: custom.smtpHost.trim(),
            smtp_port: Number(custom.smtpPort),
            smtp_security: custom.smtpSecurity,
          }
        : {}),
    };
    void onSubmit(request);
  };

  return (
    <form
      className="account-setup-form"
      autoComplete="off"
      noValidate
      onSubmit={handleSubmit}
    >
      {showProviderPicker ? (
        <div className="account-provider-grid" role="radiogroup" aria-label="邮箱服务商">
          {options.map((option) => (
            <button
              key={option.id}
              type="button"
              role="radio"
              aria-checked={provider === option.id}
              aria-disabled={option.disabled}
              data-selected={provider === option.id}
              data-disabled={option.disabled}
              onClick={() => {
                setEmail((current) =>
                  emailValueForProviderChange(current, provider, option.id),
                );
                setProvider(option.id);
                setValidationError(null);
              }}
            >
              {option.label}
            </button>
          ))}
        </div>
      ) : null}

      {configurationBlocked ? (
        <div className="account-auth-notice" role="status">
          <ShieldWarning size={19} weight="duotone" />
          <span>
            <strong>{selected?.label} 暂不能配置</strong>
            {selected?.note || "Mine Mail 暂不支持此登录方式。"}
          </span>
        </div>
      ) : (
        provider === "gmail" ? (
          <div className="account-google-auth" role="status">
            <ShieldCheck size={22} weight="duotone" />
            <span>
              <strong>通过 Google 安全登录</strong>
              <small>
                登录将在系统默认浏览器中完成。Mine Mail 不会读取你的 Google 密码，
                OAuth 令牌只保存在系统凭据库中。
              </small>
            </span>
          </div>
        ) : (
          <>
          <div className="settings-field">
            <label htmlFor={emailInputId}>邮箱地址</label>
            <span className="settings-input-shell settings-input-shell--text account-email-control inset-input-shell">
              <input
                id={emailInputId}
                ref={emailRef}
                type={providerEmailDomain ? "text" : "email"}
                inputMode="email"
                autoCapitalize="none"
                spellCheck={false}
                autoComplete="off"
                value={email}
                aria-invalid={validationError?.field === "email" || undefined}
                aria-describedby={[
                  providerEmailDomain ? emailDomainDescriptionId : null,
                  validationError?.field === "email" &&
                  validationError?.message
                    ? "account-setup-error"
                    : null,
                ]
                  .filter(Boolean)
                  .join(" ") || undefined}
                onChange={(event) => {
                  setEmail(
                    providerEmailDomain
                      ? editableEmailValue(event.target.value, provider)
                      : event.target.value,
                  );
                  setValidationError(null);
                }}
                placeholder={
                  providerEmailDomain ? "请输入邮箱账号" : "name@example.com"
                }
              />
              {providerEmailDomain ? (
                <span
                  id={emailDomainDescriptionId}
                  className="account-email-control__suffix"
                >
                  {providerEmailDomain}
                </span>
              ) : null}
            </span>
          </div>
          {selected?.note ? <p className="account-preset-note">{selected.note}</p> : null}
          <div className="settings-field">
            <label htmlFor={secretInputId}>{selected?.secretLabel}</label>
            <div className="account-secret-control">
              <span className="settings-input-shell settings-input-shell--text inset-input-shell">
                <input
                  id={secretInputId}
                  ref={secretRef}
                  type="password"
                  aria-label={selected?.secretLabel}
                  aria-invalid={validationError?.field === "secret" || undefined}
                  aria-describedby={
                    validationError?.field === "secret" &&
                    validationError?.message
                      ? "account-setup-error"
                      : undefined
                  }
                  autoComplete="off"
                  onInput={() => setValidationError(null)}
                  placeholder="请输入授权密码"
                />
              </span>
              {authorizationGuideProviders.has(provider) &&
              onOpenAuthorizationGuide ? (
                <IconButton
                  ref={authorizationGuideButtonRef}
                  className="account-authorization-guide"
                  label={`查看 ${selected?.label}授权码获取教程`}
                  tooltipOnFocus={false}
                  onClick={() => onOpenAuthorizationGuide(provider)}
                >
                  <Question size={18} weight="bold" aria-hidden="true" />
                </IconButton>
              ) : null}
            </div>
            <small>授权信息将安全保存在系统凭据库中。</small>
          </div>

          {provider === "custom" ? (
            <div className="custom-server-grid">
              <label className="settings-field">
                <span>IMAP 主机</span>
                <span className="settings-input-shell settings-input-shell--text inset-input-shell">
                  <input
                    ref={imapHostRef}
                    autoComplete="off"
                    value={custom.imapHost}
                    aria-invalid={validationError?.field === "imapHost" || undefined}
                    aria-describedby={
                      validationError?.field === "imapHost" &&
                      validationError?.message
                        ? "account-setup-error"
                        : undefined
                    }
                    onChange={(event) => {
                      setCustom((current) => ({ ...current, imapHost: event.target.value }));
                      setValidationError(null);
                    }}
                    placeholder="imap.example.com"
                  />
                </span>
              </label>
              <label className="settings-field settings-field--port">
                <span>IMAP 端口</span>
                <span className="settings-input-shell settings-input-shell--text inset-input-shell">
                  <input
                    ref={imapPortRef}
                    type="number"
                    autoComplete="off"
                    min="1"
                    max="65535"
                    value={custom.imapPort}
                    aria-invalid={validationError?.field === "imapPort" || undefined}
                    aria-describedby={
                      validationError?.field === "imapPort" &&
                      validationError?.message
                        ? "account-setup-error"
                        : undefined
                    }
                    onChange={(event) => {
                      setCustom((current) => ({ ...current, imapPort: event.target.value }));
                      setValidationError(null);
                    }}
                  />
                </span>
              </label>
              <label className="settings-field">
                <span>SMTP 主机</span>
                <span className="settings-input-shell settings-input-shell--text inset-input-shell">
                  <input
                    ref={smtpHostRef}
                    autoComplete="off"
                    value={custom.smtpHost}
                    aria-invalid={validationError?.field === "smtpHost" || undefined}
                    aria-describedby={
                      validationError?.field === "smtpHost" &&
                      validationError?.message
                        ? "account-setup-error"
                        : undefined
                    }
                    onChange={(event) => {
                      setCustom((current) => ({ ...current, smtpHost: event.target.value }));
                      setValidationError(null);
                    }}
                    placeholder="smtp.example.com"
                  />
                </span>
              </label>
              <label className="settings-field settings-field--port">
                <span>SMTP 端口</span>
                <span className="settings-input-shell settings-input-shell--text inset-input-shell">
                  <input
                    ref={smtpPortRef}
                    type="number"
                    autoComplete="off"
                    min="1"
                    max="65535"
                    value={custom.smtpPort}
                    aria-invalid={validationError?.field === "smtpPort" || undefined}
                    aria-describedby={
                      validationError?.field === "smtpPort" &&
                      validationError?.message
                        ? "account-setup-error"
                        : undefined
                    }
                    onChange={(event) => {
                      setCustom((current) => ({ ...current, smtpPort: event.target.value }));
                      setValidationError(null);
                    }}
                  />
                </span>
              </label>
              <div className="settings-field settings-field--wide">
                <span>SMTP 安全</span>
                <span className="settings-input-shell inset-input-shell">
                  <ThemedSelect
                    className="themed-select--embedded"
                    label="SMTP 安全"
                    value={custom.smtpSecurity}
                    options={smtpSecurityOptions}
                    onValueChange={(smtpSecurity) =>
                      setCustom((current) => ({ ...current, smtpSecurity }))
                    }
                  />
                </span>
              </div>
            </div>
          ) : null}
          </>
        )
      )}

      {displayedError ? (
        <p id="account-setup-error" className="settings-error" role="alert">
          {displayedError}
        </p>
      ) : null}

      <div className="account-submit-row">
        <button
          type="submit"
          className="send-button account-submit"
          disabled={configurationBlocked || submitStatus === "saving"}
        >
          {provider === "gmail" ? (
            <GoogleLogo size={18} weight="bold" />
          ) : (
            <EnvelopeSimple size={18} weight="fill" />
          )}
          {submitStatus === "saving"
            ? provider === "gmail"
              ? "等待 Google 登录…"
              : "正在验证并保存…"
            : provider === "gmail"
              ? "使用 Google 登录"
            : status?.configured
              ? "更新账户"
              : "连接邮箱"}
        </button>
        {provider === "gmail" ? (
          <small className="account-google-preview-note" role="note">
            目前处于预览测试版，如想使用 Google OAuth 登录，请联系 tantless@163.com
            添加白名单。
          </small>
        ) : null}
      </div>
    </form>
  );
}
