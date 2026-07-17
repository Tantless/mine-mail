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
          sender: "Tantless",
          subject: "A new message",
          uid: 88,
          count: 1,
          webSound: null,
        },
      });
    });

    expect(screen.getByText("Tantless")).toBeTruthy();
    expect(screen.getByText("A new message")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "打开新邮件" }));
    expect(notificationBridge.openNewMailNotification).toHaveBeenCalledWith(
      12,
      88,
    );
  });

  it("dismisses only the notification id currently on screen", async () => {
    const user = userEvent.setup();
    notificationBridge.getNewMailNotification.mockResolvedValue({
      notificationId: 14,
      sender: "Mine Mail",
      subject: "收到 2 封新邮件",
      uid: 90,
      count: 2,
      webSound: null,
    });
    render(<NewMailNotification />);

    await user.click(
      await screen.findByRole("button", { name: "关闭新邮件通知" }),
    );
    expect(notificationBridge.dismissNewMailNotification).toHaveBeenCalledWith(14);
  });
});
