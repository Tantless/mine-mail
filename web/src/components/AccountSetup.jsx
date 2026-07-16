import { useEffect, useMemo, useRef, useState } from "react";
import { EnvelopeSimple, ShieldWarning } from "@phosphor-icons/react";

const fallbackPresets = [
  { id: "163", label: "163 邮箱", secret_label: "客户端授权密码" },
  { id: "gmail", label: "Gmail", secret_label: "应用专用密码" },
  { id: "outlook", label: "Outlook", disabled: true },
  { id: "custom", label: "自定义 IMAP/SMTP", secret_label: "邮箱密码或授权密码" },
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
  };
}

export function AccountSetupForm({ presets, status, submitStatus, error, onSubmit }) {
  const options = useMemo(
    () => (presets?.length ? presets : fallbackPresets).map(normalizedPreset),
    [presets],
  );
  const initialProvider = status?.provider || options.find((item) => !item.disabled)?.id || "163";
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
    if (status?.provider) setProvider(status.provider);
  }, [status?.email, status?.provider]);

  const selected = options.find((item) => item.id === provider) || options[0];
  const outlookBlocked = provider === "outlook";
  const configurationBlocked = outlookBlocked || Boolean(selected?.disabled);

  const handleSubmit = (event) => {
    event.preventDefault();
    if (configurationBlocked || submitStatus === "saving") return;
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
    <form className="account-setup-form" onSubmit={handleSubmit}>
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
        <>
          <label className="settings-field">
            <span>邮箱地址</span>
            <span className="settings-input-shell inset-input-shell">
              <input
                type="email"
                required
                autoComplete="username"
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
                autoComplete="new-password"
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
                    min="1"
                    max="65535"
                    value={custom.smtpPort}
                    onChange={(event) =>
                      setCustom((current) => ({ ...current, smtpPort: event.target.value }))
                    }
                  />
                </span>
              </label>
              <label className="settings-field settings-field--wide">
                <span>SMTP 安全</span>
                <span className="settings-input-shell inset-input-shell">
                  <select
                    value={custom.smtpSecurity}
                    onChange={(event) =>
                      setCustom((current) => ({ ...current, smtpSecurity: event.target.value }))
                    }
                  >
                    <option value="implicit_tls">TLS</option>
                    <option value="start_tls">STARTTLS</option>
                  </select>
                </span>
              </label>
            </div>
          ) : null}
        </>
      )}

      {error ? <p className="settings-error" role="alert">{error}</p> : null}

      <button
        type="submit"
        className="send-button account-submit"
        disabled={configurationBlocked || submitStatus === "saving"}
      >
        <EnvelopeSimple size={18} weight="fill" />
        {submitStatus === "saving"
          ? "正在验证并保存…"
          : status?.configured
            ? "更新账户"
            : "连接邮箱"}
      </button>
    </form>
  );
}

export function AccountSetupPanel(props) {
  return (
    <div className="account-onboarding-layer">
      <section className="account-onboarding" aria-labelledby="account-onboarding-title">
        <span className="account-onboarding__mark">
          <EnvelopeSimple size={28} weight="duotone" />
        </span>
        <p className="eyebrow">MINE MAIL</p>
        <h1 id="account-onboarding-title">先连接你的邮箱</h1>
        <p className="account-onboarding__lead">
          完成连接后，即可收取、阅读和发送邮件。
        </p>
        <AccountSetupForm {...props} />
      </section>
    </div>
  );
}
