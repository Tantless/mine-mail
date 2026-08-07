import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const notificationBridge = vi.hoisted(() => ({
  handler: null,
  getNewMailNotification: vi.fn(),
  dismissNewMailNotification: vi.fn(),
  openNewMailNotification: vi.fn(),
  onMailEvent: vi.fn(async (_name, handler) => {
    notificationBridge.handler = handler;
    return () => {
      notificationBridge.handler = null;
    };
  }),
}));

vi.mock("../services/mailApi.js", () => ({
  mailApi: notificationBridge,
}));

import { NewMailNotification } from "./NewMailNotification.jsx";

describe("Mine Mail new mail notification surface", () => {
  beforeEach(() => {
    notificationBridge.handler = null;
    notificationBridge.getNewMailNotification.mockReset().mockResolvedValue(null);
    notificationBridge.dismissNewMailNotification.mockReset().mockResolvedValue(true);
    notificationBridge.openNewMailNotification.mockReset().mockResolvedValue(true);
    notificationBridge.onMailEvent.mockClear();
  });

  afterEach(() => cleanup());

  it("renders the themed sender and subject and opens the selected message", async () => {
    const user = userEvent.setup();
    render(<NewMailNotification />);
    await waitFor(() => expect(notificationBridge.handler).toBeTypeOf("function"));

    await act(async () => {
      notificationBridge.handler({
        payload: {
          notificationId: 12,
          sender: "产品团队",
          senderEmail: "sender@example.com",
          senderRemark: "产品团队",
          senderAvatarDataUrl: "data:image/png;base64,AQID",
          subject: "A new message",
          recipientEmail: "me@163.com",
          recipientRemark: "工作邮箱",
          count: 1,
          webSound: null,
        },
      });
    });

    expect(screen.getByText("产品团队")).toBeTruthy();
    expect(screen.getByText("sender@example.com")).toBeTruthy();
    expect(screen.getByText("A new message")).toBeTruthy();
    expect(screen.getByText("收信至 工作邮箱 · me@163.com")).toBeTruthy();
    expect(screen.getByLabelText("产品团队 的自定义头像")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "打开新邮件" }));
    expect(notificationBridge.openNewMailNotification).toHaveBeenCalledWith(12);
  });

  it("dismisses only the notification id currently on screen", async () => {
    const user = userEvent.setup();
    notificationBridge.getNewMailNotification.mockResolvedValue({
      notificationId: 14,
      sender: "Mine Mail",
      subject: "收到 2 封新邮件",
      count: 2,
      webSound: null,
    });
    render(<NewMailNotification />);

    expect(await screen.findByText("2 封新邮件 · 刚刚")).toBeTruthy();
    await user.click(
      screen.getByRole("button", { name: "关闭新邮件通知" }),
    );
    expect(notificationBridge.dismissNewMailNotification).toHaveBeenCalledWith(14);
  });

  it("shows exact batch counts through 99 and caps larger labels at 99+", async () => {
    render(<NewMailNotification />);
    await waitFor(() => expect(notificationBridge.handler).toBeTypeOf("function"));

    await act(async () => {
      notificationBridge.handler({
        payload: {
          notificationId: 15,
          sender: "Mine Mail",
          subject: "99 封批次",
          count: 99,
          webSound: null,
        },
      });
    });
    expect(screen.getByText("99 封新邮件 · 刚刚")).toBeTruthy();

    await act(async () => {
      notificationBridge.handler({
        payload: {
          notificationId: 16,
          sender: "Mine Mail",
          subject: "超过 99 封批次",
          count: 250,
          webSound: null,
        },
      });
    });
    expect(screen.getByText("99+ 封新邮件 · 刚刚")).toBeTruthy();
    expect(screen.queryByText("250 封新邮件 · 刚刚")).toBeNull();
  });

  it("restores the notification when opening fails so the user can retry", async () => {
    const user = userEvent.setup();
    notificationBridge.openNewMailNotification.mockRejectedValueOnce(
      new Error("main window unavailable"),
    );
    notificationBridge.getNewMailNotification
      .mockResolvedValueOnce(null)
      .mockRejectedValueOnce(new Error("pending state unavailable"));
    render(<NewMailNotification />);
    await waitFor(() => expect(notificationBridge.handler).toBeTypeOf("function"));

    await act(async () => {
      notificationBridge.handler({
        payload: {
          notificationId: 18,
          sender: "发件人",
          subject: "仍可重试",
          count: 1,
          webSound: null,
        },
      });
    });

    await user.click(screen.getByRole("button", { name: "打开新邮件" }));

    expect(notificationBridge.openNewMailNotification).toHaveBeenCalledWith(18);
    expect(
      await screen.findByRole("button", { name: "打开新邮件" }),
    ).toBeTruthy();
    expect(screen.getByText("仍可重试")).toBeTruthy();
    expect(notificationBridge.dismissNewMailNotification).not.toHaveBeenCalled();
  });

  it("does not revive an obsolete notification when native open returns false and none is pending", async () => {
    notificationBridge.openNewMailNotification.mockResolvedValueOnce(false);
    const user = userEvent.setup();
    render(<NewMailNotification />);
    await waitFor(() => expect(notificationBridge.handler).toBeTypeOf("function"));

    await act(async () => {
      notificationBridge.handler({
        payload: {
          notificationId: 19,
          sender: "旧发件人",
          subject: "已由其他窗口处理",
          count: 1,
          webSound: null,
        },
      });
    });
    await user.click(screen.getByRole("button", { name: "打开新邮件" }));
    await waitFor(() =>
      expect(notificationBridge.getNewMailNotification).toHaveBeenCalledTimes(2),
    );

    expect(screen.queryByText("已由其他窗口处理")).toBeNull();
  });

  it("does not let a stale open result hide a newer notification", async () => {
    let resolveOpen;
    notificationBridge.openNewMailNotification.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOpen = resolve;
      }),
    );
    const user = userEvent.setup();
    render(<NewMailNotification />);
    await waitFor(() => expect(notificationBridge.handler).toBeTypeOf("function"));

    await act(async () => {
      notificationBridge.handler({
        payload: {
          notificationId: 21,
          sender: "旧发件人",
          subject: "旧通知",
          count: 1,
          webSound: null,
        },
      });
    });
    await user.click(screen.getByRole("button", { name: "打开新邮件" }));

    await act(async () => {
      notificationBridge.handler({
        payload: {
          notificationId: 22,
          sender: "新发件人",
          subject: "新通知",
          count: 1,
          webSound: null,
        },
      });
      resolveOpen(false);
    });

    expect(await screen.findByText("新通知")).toBeTruthy();
    expect(screen.queryByText("旧通知")).toBeNull();
  });

  it("orders, deduplicates, and invokes decimal string ids without losing precision", async () => {
    const lowerId = "90071992547409930";
    const higherId = "90071992547409931";
    const user = userEvent.setup();
    render(<NewMailNotification />);
    await waitFor(() => expect(notificationBridge.handler).toBeTypeOf("function"));

    await act(async () => {
      notificationBridge.handler({
        payload: {
          notificationId: lowerId,
          sender: "旧发件人",
          subject: "大整数旧通知",
          count: 1,
          webSound: null,
        },
      });
      notificationBridge.handler({
        payload: {
          notificationId: higherId,
          sender: "新发件人",
          subject: "大整数新通知",
          count: 1,
          webSound: null,
        },
      });
      notificationBridge.handler({
        payload: {
          notificationId: `0${higherId}`,
          sender: "重复发件人",
          subject: "数值相同的重复通知",
          count: 1,
          webSound: null,
        },
      });
    });

    expect(await screen.findByText("大整数新通知")).toBeTruthy();
    expect(screen.queryByText("大整数旧通知")).toBeNull();
    expect(screen.queryByText("数值相同的重复通知")).toBeNull();

    await user.click(screen.getByRole("button", { name: "打开新邮件" }));
    expect(notificationBridge.openNewMailNotification).toHaveBeenCalledWith(
      higherId,
    );
  });

  it("restores a notification when native dismissal reports a stale id", async () => {
    notificationBridge.dismissNewMailNotification.mockResolvedValueOnce(false);
    notificationBridge.getNewMailNotification
      .mockResolvedValueOnce({
        notificationId: 24,
        sender: "待处理发件人",
        subject: "仍待处理",
        count: 1,
        webSound: null,
      })
      .mockResolvedValueOnce({
        notificationId: 24,
        sender: "待处理发件人",
        subject: "仍待处理",
        count: 1,
        webSound: null,
      });
    const user = userEvent.setup();
    render(<NewMailNotification />);

    expect(await screen.findByText("仍待处理")).toBeTruthy();
    await user.click(
      screen.getByRole("button", { name: "关闭新邮件通知" }),
    );

    expect(await screen.findByText("仍待处理")).toBeTruthy();
    expect(notificationBridge.dismissNewMailNotification).toHaveBeenCalledWith(24);
  });

  it("limits retries per notification and resets the budget for a newer one", async () => {
    let pending = {
      notificationId: 30,
      sender: "待处理发件人",
      subject: "第一封待关闭邮件",
      count: 1,
      webSound: null,
    };
    notificationBridge.dismissNewMailNotification.mockResolvedValue(false);
    notificationBridge.getNewMailNotification.mockImplementation(async () => pending);
    const user = userEvent.setup();
    render(<NewMailNotification />);

    expect(await screen.findByText("第一封待关闭邮件")).toBeTruthy();
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await user.click(
        screen.getByRole("button", { name: "关闭新邮件通知" }),
      );
      expect(await screen.findByText("第一封待关闭邮件")).toBeTruthy();
    }
    await user.click(
      screen.getByRole("button", { name: "关闭新邮件通知" }),
    );
    await waitFor(() =>
      expect(screen.queryByText("第一封待关闭邮件")).toBeNull(),
    );

    pending = {
      notificationId: 31,
      sender: "另一位发件人",
      subject: "第二封待关闭邮件",
      count: 1,
      webSound: null,
    };
    await act(async () => {
      notificationBridge.handler({ payload: pending });
    });
    expect(await screen.findByText("第二封待关闭邮件")).toBeTruthy();
    await user.click(
      screen.getByRole("button", { name: "关闭新邮件通知" }),
    );

    expect(await screen.findByText("第二封待关闭邮件")).toBeTruthy();
    expect(notificationBridge.dismissNewMailNotification).toHaveBeenCalledTimes(5);
  });
});
