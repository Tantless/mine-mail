const DEFAULT_INTERNAL_MESSAGE =
  "Mine Mail 内部处理失败，请重试；如果仍然失败，请重启应用。";

const translatedErrors = [
  {
    pattern: /QQ preset requires an @qq\.com address/i,
    kind: "input",
    message: "请检查输入：QQ 邮箱地址必须以 @qq.com 结尾。",
  },
  {
    pattern: /163 preset requires an @163\.com address/i,
    kind: "input",
    message: "请检查输入：163 邮箱地址必须以 @163.com 结尾。",
  },
  {
    pattern: /Custom accounts require explicit IMAP and SMTP settings/i,
    kind: "input",
    message: "请检查输入：自定义邮箱需要填写完整的 IMAP 和 SMTP 设置。",
  },
  {
    pattern: /valid IMAP port is required/i,
    kind: "input",
    message: "请检查输入：请输入有效的 IMAP 端口。",
  },
  {
    pattern: /valid SMTP port is required/i,
    kind: "input",
    message: "请检查输入：请输入有效的 SMTP 端口。",
  },
  {
    pattern: /SMTP security must be implicit TLS or STARTTLS/i,
    kind: "input",
    message: "请检查输入：SMTP 安全方式只能选择 TLS 或 STARTTLS。",
  },
  {
    pattern: /account settings are invalid/i,
    kind: "input",
    message: "请检查输入：邮箱账户设置无效。",
  },
  {
    pattern: /Theme schedule times must use HH:MM format/i,
    kind: "input",
    message: "请检查输入：定时切换主题的时间请使用 HH:MM 格式（例如 06:00）。",
  },
  {
    pattern: /Theme schedule times must be ordered day, dusk, then night/i,
    kind: "input",
    message: "请检查输入：定时切换主题需要按日间、黄昏、夜间的顺序排列时间。",
  },
  {
    pattern: /Recipient confirmation did not match/i,
    kind: "operation",
    message: "操作未完成：收件人信息已变化，请重新确认后发送。",
  },
  {
    pattern: /permanent-delete plan is invalid or expired/i,
    kind: "operation",
    message: "操作未完成：永久删除确认已失效，请重新确认。",
  },
  {
    pattern: /continuation cursor is invalid or expired/i,
    kind: "operation",
    message: "操作未完成：邮件列表状态已变化，请刷新后重试。",
  },
  {
    pattern: /(?:account|message|attachment|Outbox) identifier is invalid/i,
    kind: "operation",
    message: "操作未完成：当前内容已失效，请刷新后重试。",
  },
  {
    pattern: /No mail account is selected/i,
    kind: "operation",
    message: "操作未完成：请先选择一个邮箱账户。",
  },
  {
    pattern: /three-account limit/i,
    kind: "operation",
    message: "操作未完成：Mine Mail 最多只能连接三个邮箱账户。",
  },
  {
    pattern: /Mail sending cannot start while Mine Mail is exiting/i,
    kind: "operation",
    message: "操作未完成：Mine Mail 正在退出，暂时不能开始发送邮件。",
  },
  {
    pattern: /selected attachment could not be accessed/i,
    kind: "operation",
    message: "操作未完成：无法读取所选附件，请重新选择文件。",
  },
  {
    pattern: /link is invalid/i,
    kind: "operation",
    message: "操作未完成：该链接无效。",
  },
  {
    pattern: /link is not safe to open/i,
    kind: "operation",
    message: "操作未完成：为保护你的安全，Mine Mail 已阻止打开该链接。",
  },
  {
    pattern: /link has no recipient/i,
    kind: "operation",
    message: "操作未完成：该邮件链接没有收件人。",
  },
  {
    pattern: /link type is not supported/i,
    kind: "operation",
    message: "操作未完成：Mine Mail 不支持打开这种链接。",
  },
  {
    pattern: /Sending failed before delivery was confirmed/i,
    kind: "network",
    message: "邮箱服务未确认本次投递，可以安全重试。",
  },
  {
    pattern: /Delivery could not be confirmed/i,
    kind: "network",
    message: "投递结果仍待确认，请先到邮箱服务商的“已发送”文件夹核对。",
  },
  {
    pattern: /(?:newer )?ambiguous attempt|delivery result (?:is )?unknown/i,
    kind: "network",
    message: "投递结果仍待确认，请先到邮箱服务商的“已发送”文件夹核对。",
  },
  {
    pattern: /Permanent rejection/i,
    kind: "input",
    message: "邮箱服务器拒绝了这次投递，请检查收件人和账户设置。",
  },
  {
    pattern: /Temporary failure/i,
    kind: "network",
    message: "邮箱服务暂时未能完成操作，请稍后重试。",
  },
  {
    pattern: /credential|authorization|authentication|sign in|login/i,
    kind: "account",
    message: "账户授权需要处理，请重新登录或更新授权信息。",
  },
  {
    pattern: /network|connection|timed? out|timeout|offline|could not reach/i,
    kind: "network",
    message: "网络或邮箱服务暂时不可用，请检查连接后重试。",
  },
];

const untranslatedEnglishPattern =
  /\b(?:the|this|could|cannot|failed|failure|invalid|required|requires|must|unavailable|unsupported|before|after|while|without|nothing|selected|saved|existing|temporary|permanent)\b/i;
const chineseTextPattern = /[\u3400-\u9fff]/u;

function rawErrorMessage(error) {
  if (typeof error === "string") return error.trim();
  if (error?.message && typeof error.message === "string") {
    return error.message.trim();
  }
  return "";
}

function normalizedFallback(fallback) {
  const value = rawErrorMessage(fallback);
  if (
    !value ||
    !chineseTextPattern.test(value) ||
    untranslatedEnglishPattern.test(value)
  ) {
    return DEFAULT_INTERNAL_MESSAGE;
  }
  return value;
}

function inferChineseKind(message) {
  if (/凭据|授权|登录|OAuth/u.test(message)) return "account";
  if (/网络|连接|离线|服务器|邮箱服务/u.test(message)) return "network";
  if (/请(?:输入|选择|检查|确认)|不正确|无效|不能|必须|已失效|不存在/u.test(message)) {
    return "operation";
  }
  return "internal";
}

export function describeUserFacingError(error, fallback) {
  const rawMessage = rawErrorMessage(error).replace(/^Error:\s*/i, "");
  const translated = translatedErrors.find(({ pattern }) =>
    pattern.test(rawMessage),
  );
  if (translated) {
    return { kind: translated.kind, message: translated.message };
  }

  if (
    rawMessage &&
    chineseTextPattern.test(rawMessage) &&
    !untranslatedEnglishPattern.test(rawMessage)
  ) {
    return {
      kind: inferChineseKind(rawMessage),
      message: rawMessage,
    };
  }

  const fallbackMessage = normalizedFallback(fallback);
  if (fallbackMessage === DEFAULT_INTERNAL_MESSAGE) {
    return { kind: "internal", message: fallbackMessage };
  }
  const detail = fallbackMessage.replace(/[。！？!?]+$/u, "");
  return {
    kind: "internal",
    message: `Mine Mail 内部处理失败：${detail}${
      /请重试/u.test(detail) ? "；如果仍然失败" : "。请重试；如果仍然失败"
    }，请重启应用。`,
  };
}

export function userFacingErrorMessage(error, fallback) {
  return describeUserFacingError(error, fallback).message;
}

export function toUserFacingError(error, fallback) {
  if (error?.name === "UserFacingError" && error.message) return error;
  const presentation = describeUserFacingError(error, fallback);
  const userError = new Error(presentation.message, { cause: error });
  userError.name = "UserFacingError";
  userError.kind = presentation.kind;
  return userError;
}

export const __testing = {
  DEFAULT_INTERNAL_MESSAGE,
  rawErrorMessage,
};
