import { describe, expect, it } from "vitest";
import {
  describeUserFacingError,
  toUserFacingError,
  userFacingErrorMessage,
} from "./userFacingError.js";

describe("user-facing error boundary", () => {
  it("translates a provider input rejection and identifies it as input", () => {
    expect(
      describeUserFacingError(
        "The QQ preset requires an @qq.com address.",
        "邮箱账户连接没有完成",
      ),
    ).toEqual({
      kind: "input",
      message: "请检查输入：QQ 邮箱地址必须以 @qq.com 结尾。",
    });
  });

  it("distinguishes account authorization and network failures", () => {
    expect(
      describeUserFacingError("Saved credential is invalid; sign in again."),
    ).toEqual({
      kind: "account",
      message: "账户授权需要处理，请重新登录或更新授权信息。",
    });
    expect(
      describeUserFacingError("SMTP connection timed out before greeting."),
    ).toEqual({
      kind: "network",
      message: "网络或邮箱服务暂时不可用，请检查连接后重试。",
    });
  });

  it("never exposes an unknown English diagnostic as interface copy", () => {
    expect(
      userFacingErrorMessage(
        new Error("SQLite write failed at an internal bridge"),
        "草稿保存没有完成",
      ),
    ).toBe(
      "Mine Mail 内部处理失败：草稿保存没有完成。请重试；如果仍然失败，请重启应用。",
    );
    expect(userFacingErrorMessage("unexpected bridge explosion")).toBe(
      "Mine Mail 内部处理失败，请重试；如果仍然失败，请重启应用。",
    );
  });

  it("preserves already actionable Chinese copy", () => {
    expect(
      userFacingErrorMessage("请检查输入：邮箱地址格式不正确。"),
    ).toBe("请检查输入：邮箱地址格式不正确。");
  });

  it("creates a categorized Error for API callers", () => {
    const error = toUserFacingError(
      "Recipient confirmation did not match",
      "发送邮件没有完成",
    );
    expect(error).toBeInstanceOf(Error);
    expect(error.name).toBe("UserFacingError");
    expect(error.kind).toBe("operation");
    expect(error.message).toBe(
      "操作未完成：收件人信息已变化，请重新确认后发送。",
    );
  });
});
