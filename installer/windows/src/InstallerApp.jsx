import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowRight,
  Desktop,
  EnvelopeSimpleOpen,
  FolderOpen,
  Minus,
  Package,
  PaperPlaneTilt,
  Play,
  Power,
  WarningCircle,
  X,
} from "@phosphor-icons/react";
import foxLogo from "./assets/mine-mail-fox.png";
import {
  INSTALLER_STEPS,
  defaultPreviewInfo,
  stepTone,
} from "./installerState";

const isTauriRuntime =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const previewDelay = (milliseconds) =>
  new Promise((resolve) => window.setTimeout(resolve, milliseconds));

export function normalizeSelectedInstallDir(path) {
  return /^[A-Za-z]:[\\/]$/.test(path) ? `${path}MineMail` : path;
}

function StepIcon({ step }) {
  if (step === "ready") {
    return <EnvelopeSimpleOpen data-step-icon="ready" weight="duotone" />;
  }
  if (step === "installing") {
    return <Package data-step-icon="installing" weight="duotone" />;
  }
  return <PaperPlaneTilt data-step-icon="success" weight="duotone" />;
}

function StepRail({ installerState }) {
  return (
    <ol className="step-rail" aria-label="安装进度">
      {INSTALLER_STEPS.map((step, index) => {
        const tone = stepTone(installerState, index);
        return (
          <li className={`step-item step-item--${tone}`} key={step.id}>
            <span className="step-marker" aria-hidden="true">
              <StepIcon step={step.id} />
            </span>
            <span className="step-copy">
              <strong>{step.label}</strong>
            </span>
          </li>
        );
      })}
    </ol>
  );
}

function WindowControls({ installerState, onCloseNotice }) {
  const minimize = async () => {
    if (isTauriRuntime) await invoke("minimize_installer");
  };

  const close = async () => {
    if (installerState === "installing") {
      onCloseNotice();
      return;
    }
    if (isTauriRuntime) await invoke("close_installer");
  };

  return (
    <div className="window-controls" aria-label="窗口控制">
      <button
        className="window-control"
        type="button"
        aria-label="最小化"
        onClick={minimize}
      >
        <Minus weight="bold" />
      </button>
      <button
        className="window-control window-control--close"
        type="button"
        aria-label="关闭"
        onClick={close}
      >
        <X weight="bold" />
      </button>
    </div>
  );
}

function ReadyPanel({ installDir, payloadAvailable, onBrowse, onInstall }) {
  return (
    <section className="content-panel ready-panel" aria-labelledby="ready-title">
      <h1 id="ready-title">安装 Mine Mail</h1>

      <div className="path-card">
        <span className="path-icon" aria-hidden="true">
          <FolderOpen weight="duotone" />
        </span>
        <span className="path-copy">
          <span>安装位置</span>
          <strong title={installDir}>{installDir}</strong>
        </span>
        <button className="soft-button" type="button" onClick={onBrowse}>
          更改
        </button>
      </div>

      {!payloadAvailable && (
        <p className="payload-warning" role="alert">
          当前是未嵌入安装载荷的开发预览，无法执行真实安装。
        </p>
      )}

      <div className="primary-row">
        <button
          className="primary-button"
          type="button"
          disabled={!payloadAvailable}
          onClick={onInstall}
        >
          安装
          <ArrowRight weight="bold" aria-hidden="true" />
        </button>
      </div>
    </section>
  );
}

function InstallingPanel({ stage }) {
  return (
    <section
      className="content-panel installing-panel"
      aria-labelledby="installing-title"
      aria-live="polite"
    >
      <h1 id="installing-title">正在安装 Mine Mail</h1>
      <p className="stage-message">{stage.message}</p>
      <div
        className="progress-track"
        role="progressbar"
        aria-label="安装进行中"
        aria-valuetext={stage.message}
      >
        <span className="progress-runner" />
      </div>
    </section>
  );
}

function SuccessPanel({
  installOptions,
  optionPending,
  optionError,
  onOptionChange,
  onLaunch,
}) {
  return (
    <section
      className="content-panel success-panel"
      aria-labelledby="success-title"
      aria-live="polite"
    >
      <h1 id="success-title">安装完成</h1>

      <fieldset className="finish-options" aria-label="安装完成选项">
        <label className="finish-option">
          <span className="finish-option__icon" aria-hidden="true">
            <Desktop weight="duotone" />
          </span>
          <span className="finish-option__copy">
            <strong>桌面快捷方式</strong>
          </span>
          <input
            type="checkbox"
            checked={installOptions.desktopShortcut}
            disabled={optionPending !== null}
            onChange={(event) =>
              onOptionChange("desktopShortcut", event.target.checked)
            }
          />
          <span className="option-switch" aria-hidden="true">
            <span />
          </span>
        </label>

        <label className="finish-option">
          <span className="finish-option__icon" aria-hidden="true">
            <Power weight="duotone" />
          </span>
          <span className="finish-option__copy">
            <strong>开机时启动</strong>
          </span>
          <input
            type="checkbox"
            checked={installOptions.autostart}
            disabled={optionPending !== null}
            onChange={(event) =>
              onOptionChange("autostart", event.target.checked)
            }
          />
          <span className="option-switch" aria-hidden="true">
            <span />
          </span>
        </label>
      </fieldset>

      {optionError && (
        <p className="option-error" role="alert">
          {optionError}
        </p>
      )}

      <div className="completion-footer">
        <button
          className="primary-button primary-button--launch"
          type="button"
          onClick={onLaunch}
        >
          <Play weight="fill" aria-hidden="true" />
          打开 Mine Mail
        </button>
      </div>
    </section>
  );
}

export function ErrorPanel({
  message,
  defaultInstallDir,
  onChooseAnother,
  onUseDefault,
}) {
  return (
    <section
      className="content-panel error-panel"
      aria-labelledby="error-title"
      aria-live="assertive"
    >
      <div className="error-symbol" aria-hidden="true">
        <WarningCircle weight="duotone" />
      </div>
      <h1 id="error-title">安装未完成</h1>
      <p className="stage-message">安装已停止，没有改用其他位置继续。</p>
      <p className="error-message">{message}</p>
      <p className="error-recovery">
        建议改用默认位置安装，或重新选择一个可写文件夹。
        <strong title={defaultInstallDir}>{defaultInstallDir}</strong>
      </p>
      <div className="error-actions">
        <button
          className="secondary-button"
          type="button"
          onClick={onChooseAnother}
        >
          重新选择位置
        </button>
        <button
          className="primary-button primary-button--compact"
          type="button"
          onClick={onUseDefault}
        >
          使用默认位置
        </button>
      </div>
    </section>
  );
}

export default function InstallerApp() {
  const [info, setInfo] = useState(defaultPreviewInfo);
  const [installDir, setInstallDir] = useState(
    defaultPreviewInfo().defaultInstallDir,
  );
  const [installerState, setInstallerState] = useState("ready");
  const [stage, setStage] = useState({
    label: "准备安装文件",
    message: "正在检查安装环境…",
  });
  const [errorMessage, setErrorMessage] = useState("");
  const [showCloseNotice, setShowCloseNotice] = useState(false);
  const [installOptions, setInstallOptions] = useState({
    desktopShortcut: true,
    autostart: false,
  });
  const [optionPending, setOptionPending] = useState(null);
  const [optionError, setOptionError] = useState("");

  useEffect(() => {
    const preventContextMenu = (event) => event.preventDefault();
    window.addEventListener("contextmenu", preventContextMenu);

    if (!isTauriRuntime) {
      return () => window.removeEventListener("contextmenu", preventContextMenu);
    }

    let disposed = false;
    const unlisteners = [];

    invoke("installer_info")
      .then((nextInfo) => {
        if (disposed) return;
        setInfo(nextInfo);
        setInstallDir(nextInfo.defaultInstallDir);
      })
      .catch((error) => {
        if (disposed) return;
        setInfo((current) => ({ ...current, payloadAvailable: false }));
        setErrorMessage(String(error));
      });

    listen("installer://stage", ({ payload }) => {
      if (!disposed) setStage(payload);
    }).then((unlisten) => {
      if (disposed) unlisten();
      else unlisteners.push(unlisten);
    });

    listen("installer://close-blocked", () => {
      if (!disposed) setShowCloseNotice(true);
    }).then((unlisten) => {
      if (disposed) unlisten();
      else unlisteners.push(unlisten);
    });

    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => unlisten());
      window.removeEventListener("contextmenu", preventContextMenu);
    };
  }, []);

  const versionLabel = useMemo(() => `v${info.version}`, [info.version]);

  const chooseInstallDirectory = async () => {
    if (!isTauriRuntime) return false;
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择 Mine Mail 的安装位置",
      defaultPath: installDir,
    });
    if (typeof selected !== "string") return false;
    setInstallDir(normalizeSelectedInstallDir(selected));
    return true;
  };

  const browse = () => void chooseInstallDirectory();

  const chooseAnotherAfterFailure = async () => {
    if (await chooseInstallDirectory()) setInstallerState("ready");
  };

  const useDefaultAfterFailure = () => {
    setInstallDir(info.defaultInstallDir);
    setInstallerState("ready");
  };

  const runPreviewInstallation = async () => {
    const stages = [
      ["准备安装文件", "正在检查安装环境…", 520],
      ["写入应用文件", "正在安装 Mine Mail…", 1500],
      ["确认安装结果", "正在完成最后检查…", 650],
    ];
    for (const [label, message, duration] of stages) {
      setStage({ label, message });
      await previewDelay(duration);
    }
    return {
      installDir,
      desktopShortcutEnabled: true,
      autostartEnabled: false,
    };
  };

  const install = async () => {
    setInstallerState("installing");
    setErrorMessage("");
    setStage({ label: "准备安装文件", message: "正在检查安装环境…" });

    try {
      const result = isTauriRuntime
        ? await invoke("start_install", { installDir })
        : await runPreviewInstallation();
      setInstallDir(result.installDir);
      setInstallOptions({
        desktopShortcut: result.desktopShortcutEnabled ?? true,
        autostart: result.autostartEnabled ?? false,
      });
      setInstallerState("success");
    } catch (error) {
      setErrorMessage(String(error));
      setInstallerState("error");
    }
  };

  const launch = async () => {
    if (isTauriRuntime) await invoke("launch_installed_app");
  };

  const updateInstallOption = async (option, enabled) => {
    const previous = installOptions[option];
    const command =
      option === "desktopShortcut"
        ? "set_desktop_shortcut_enabled"
        : "set_autostart_enabled";

    setOptionError("");
    setOptionPending(option);
    setInstallOptions((current) => ({ ...current, [option]: enabled }));

    try {
      if (isTauriRuntime) {
        const applied = await invoke(command, { enabled });
        setInstallOptions((current) => ({ ...current, [option]: applied }));
      }
    } catch (error) {
      setInstallOptions((current) => ({ ...current, [option]: previous }));
      setOptionError(`设置没有保存：${String(error)}`);
    } finally {
      setOptionPending(null);
    }
  };

  return (
    <div className="installer-root">
      <div className="window-surface">
        <div className="drag-region" data-tauri-drag-region aria-hidden="true" />
        <WindowControls
          installerState={installerState}
          onCloseNotice={() => setShowCloseNotice(true)}
        />

        <aside className="brand-panel">
          <div className="brand-lockup">
            <img className="brand-fox" src={foxLogo} alt="Mine Mail 狐狸标志" />
            <div>
              <strong>Mine Mail</strong>
              <span>多账户邮件客户端</span>
            </div>
          </div>
          <StepRail installerState={installerState} />
          <div className="version-chip">{versionLabel}</div>
        </aside>

        <main className="installer-workspace">
          {installerState === "ready" && (
            <ReadyPanel
              installDir={installDir}
              payloadAvailable={info.payloadAvailable}
              onBrowse={browse}
              onInstall={install}
            />
          )}
          {installerState === "installing" && <InstallingPanel stage={stage} />}
          {installerState === "success" && (
            <SuccessPanel
              installOptions={installOptions}
              optionPending={optionPending}
              optionError={optionError}
              onOptionChange={updateInstallOption}
              onLaunch={launch}
            />
          )}
          {installerState === "error" && (
            <ErrorPanel
              message={errorMessage}
              defaultInstallDir={info.defaultInstallDir}
              onChooseAnother={chooseAnotherAfterFailure}
              onUseDefault={useDefaultAfterFailure}
            />
          )}
        </main>

        {showCloseNotice && (
          <div className="dialog-backdrop" role="presentation">
            <section
              className="close-dialog"
              role="dialog"
              aria-modal="true"
              aria-labelledby="close-dialog-title"
            >
              <span className="dialog-icon" aria-hidden="true">
                <Package weight="duotone" />
              </span>
              <h2 id="close-dialog-title">安装正在进行</h2>
              <p>为了避免留下不完整的文件，请等待当前安装结束。你可以先最小化窗口。</p>
              <button
                className="primary-button primary-button--compact"
                type="button"
                autoFocus
                onClick={() => setShowCloseNotice(false)}
              >
                继续安装
              </button>
            </section>
          </div>
        )}
      </div>
    </div>
  );
}
