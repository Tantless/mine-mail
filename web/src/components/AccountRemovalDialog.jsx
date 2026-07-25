import { ShieldWarning, Trash, X } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import { IconButton } from "./IconButton.jsx";

export function AccountRemovalDialog({
  account,
  isRemoving = false,
  onCancel,
  onConfirm,
}) {
  const [deleteLocalData, setDeleteLocalData] = useState(false);

  useEffect(() => {
    setDeleteLocalData(false);
  }, [account?.accountId]);

  if (!account) return null;
  const isGoogle =
    account.provider === "gmail" ||
    account.authentication === "google_oauth";

  const confirmRemoval = (revokeGoogleAuthorization) => {
    onConfirm({
      revokeGoogleAuthorization,
      deleteLocalData,
    });
  };

  return (
    <div className="confirm-layer">
      <section
        className="confirm-dialog account-removal-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="account-removal-title"
      >
        <header>
          <span className="confirm-dialog__icon account-removal-dialog__icon">
            <ShieldWarning size={23} weight="duotone" />
          </span>
          <IconButton
            label="取消移除账户"
            onClick={onCancel}
            disabled={isRemoving}
          >
            <X size={18} />
          </IconButton>
        </header>

        <h2 id="account-removal-title">
          {isGoogle ? "移除 Gmail 账户？" : "移除邮箱账户？"}
        </h2>
        <p className="account-removal-dialog__address">{account.email}</p>

        {isGoogle ? (
          <div className="account-removal-dialog__explanation">
            <strong>“撤销授权并移除”会完成：</strong>
            <ul>
              <li>先请求 Google 撤销 Mine Mail 的 OAuth 授权；</li>
              <li>再删除操作系统凭据库中的令牌和应用内账户记录；</li>
              <li>本地邮件缓存仅在勾选下方选项后删除。</li>
            </ul>
            <p>
              “仅断开”不会撤销 Google 账户中的 Mine Mail 授权，并会保留本地缓存。
            </p>
          </div>
        ) : (
          <div className="account-removal-dialog__explanation">
            <p>移除后，Mine Mail 会删除系统凭据和应用内账户记录。</p>
          </div>
        )}

        <label className="account-removal-dialog__cache-option">
          <input
            type="checkbox"
            checked={deleteLocalData}
            onChange={(event) => setDeleteLocalData(event.target.checked)}
            disabled={isRemoving}
          />
          <span>
            <strong>同时删除本地邮件缓存</strong>
            <small>
              删除该账户的 SQLite 邮件、草稿与发件队列缓存；此操作不可恢复。
            </small>
          </span>
        </label>

        <footer>
          <button
            type="button"
            className="secondary-button"
            onClick={onCancel}
            disabled={isRemoving}
          >
            取消
          </button>
          {isGoogle ? (
            <button
              type="button"
              className="secondary-button"
              onClick={() =>
                onConfirm({
                  revokeGoogleAuthorization: false,
                  deleteLocalData: false,
                })
              }
              disabled={isRemoving}
            >
              仅断开
            </button>
          ) : null}
          <button
            type="button"
            className="danger-button"
            onClick={() => confirmRemoval(isGoogle)}
            disabled={isRemoving}
          >
            <Trash size={17} />
            {isRemoving
              ? "正在处理…"
              : isGoogle
                ? "撤销授权并移除"
                : "移除账户"}
          </button>
        </footer>
      </section>
    </div>
  );
}
