import { useEffect, useMemo, useRef, useState } from "react";
import { EnvelopeSimple, GoogleLogo, ShieldCheck, ShieldWarning } from "@phosphor-icons/react";
import { ThemedSelect } from "./ThemedSelect.jsx";

const fallbackPresets = [
  { id: "163", label: "163 邮箱", secret_label: "客户端授权密码" },
  { id: "gmail", label: "Gmail", oauth: true, secret_label: "Google OAuth" },
  { id: "outlook", label: "Outlook", disabled: true },
  { id: "custom", label: "自定义 IMAP/SMTP", secret_label: "邮箱密码或授权密码" },
];

const smtpSecurityOptions = [
  { value: "implicit_tls", label: "TLS" },
  { value: "start_tls", label: "STARTTLS" },
];

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

export function AccountSetupForm({
  presets,
  status,
  submitStatus,
  error,
  onSubmit,
  onGoogle,
  initialProvider: requestedInitialProvider,
  showProviderPicker = true,
}) {
  const options = useMemo(
    () => (presets?.length ? presets : fallbackPresets).map(normalizedPreset),
    [presets],
  );
  const initialProvider =
    requestedInitialProvider ||
    status?.provider ||
    options.find((item) => !item.disabled)?.id ||
    "163";
  const [provider, setProvider] = useState(initialProvider);
  const [email, setEmail] = useState(status?.email || "");
  const [custom, setCustom] = useState({
    imapHost: "",
    imapPort: 993,
    smtpHost: "",
    smtpPort: 465,
    smtpSecurity: "implicit_tls",
  });
  const secretRef = useRef(null);

  useEffect(() => {
    if (status?.email) setEmail(status.email);
    if (requestedInitialProvider) setProvider(requestedInitialProvider);
    else if (status?.provider) setProvider(status.provider);
  }, [requestedInitialProvider, status?.email, status?.provider]);

  const selected = options.find((item) => item.id === provider) || options[0];
  const outlookBlocked = provider === "outlook";
  const configurationBlocked = outlookBlocked || Boolean(selected?.disabled);

  const handleSubmit = (event) => {
    event.preventDefault();
    if (configurationBlocked || submitStatus === "saving") return;
    if (provider === "gmail") {
      void onGoogle?.();
      return;
    }
    const secret = secretRef.current?.value || "";
    if (secretRef.current) secretRef.current.value = "";
    const request = {
      provider,
      email: email.trim(),
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
    <form className="account-setup-form" autoComplete="off" onSubmit={handleSubmit}>
      {showProviderPicker ? (
        <div className="account-provider-grid" role="radiogroup" aria-label="邮箱服务商">
          {options.map((option) => (
            <button
              key={option.id}
              type="button"
              role="radio"
              aria-checked={provider === option.id}
              aria-disabled={option.disabled || option.id === "outlook"}
              data-selected={provider === option.id}
              data-disabled={option.disabled || option.id === "outlook"}
              onClick={() => setProvider(option.id)}
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
            <strong>{outlookBlocked ? "Outlook 暂不能配置" : `${selected?.label} 暂不能配置`}</strong>
            {selected?.note ||
              (outlookBlocked
                ? "Mine Mail 暂不支持 Outlook 登录，请选择其他邮箱服务。"
                : "Mine Mail 暂不支持此登录方式。")}
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
          <label className="settings-field">
            <span>邮箱地址</span>
            <span className="settings-input-shell inset-input-shell">
              <input
                type="email"
                required
                autoComplete="off"
                value={email}
                onChange={(event) => setEmail(event.target.value)}
                placeholder="name@example.com"
              />
            </span>
          </label>
          {selected?.note ? <p className="account-preset-note">{selected.note}</p> : null}
          <label className="settings-field">
            <span>{selected?.secretLabel}</span>
            <span className="settings-input-shell inset-input-shell">
              <input
                ref={secretRef}
                type="password"
                aria-label={selected?.secretLabel}
                required
                autoComplete="off"
                placeholder="请输入授权密码"
              />
            </span>
            <small>授权信息将安全保存在系统凭据库中。</small>
          </label>

          {provider === "custom" ? (
            <div className="custom-server-grid">
              <label className="settings-field">
                <span>IMAP 主机</span>
                <span className="settings-input-shell inset-input-shell">
                  <input
                    required
                    autoComplete="off"
                    value={custom.imapHost}
                    onChange={(event) =>
                      setCustom((current) => ({ ...current, imapHost: event.target.value }))
                    }
                    placeholder="imap.example.com"
                  />
                </span>
              </label>
              <label className="settings-field settings-field--port">
                <span>IMAP 端口</span>
                <span className="settings-input-shell inset-input-shell">
                  <input
                    required
                    type="number"
                    autoComplete="off"
                    min="1"
                    max="65535"
                    value={custom.imapPort}
                    onChange={(event) =>
                      setCustom((current) => ({ ...current, imapPort: event.target.value }))
                    }
                  />
                </span>
              </label>
              <label className="settings-field">
                <span>SMTP 主机</span>
                <span className="settings-input-shell inset-input-shell">
                  <input
                    required
                    autoComplete="off"
                    value={custom.smtpHost}
                    onChange={(event) =>
                      setCustom((current) => ({ ...current, smtpHost: event.target.value }))
                    }
                    placeholder="smtp.example.com"
                  />
                </span>
              </label>
              <label className="settings-field settings-field--port">
                <span>SMTP 端口</span>
                <span className="settings-input-shell inset-input-shell">
                  <input
                    required
                    type="number"
                    autoComplete="off"
                    min="1"
                    max="65535"
                    value={custom.smtpPort}
                    onChange={(event) =>
                      setCustom((current) => ({ ...current, smtpPort: event.target.value }))
                    }
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

      {error ? <p className="settings-error" role="alert">{error}</p> : null}

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
    </form>
  );
}
