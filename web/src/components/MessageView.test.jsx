import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MessageView } from "./MessageView.jsx";

vi.mock("./ReaderIdleExperience.jsx", () => ({
  ReaderIdleExperience: () => <div data-testid="reader-idle-experience" />,
}));

describe("MessageView idle experience", () => {
  it("replaces the instructional placeholder and unmounts as soon as a message opens", () => {
    const { rerender } = render(<MessageView message={null} />);

    expect(screen.getByTestId("reader-idle-experience")).toBeTruthy();
    expect(screen.queryByText("选择一封邮件开始阅读")).toBeNull();
    expect(screen.queryByText("背景会在这里多留一点呼吸")).toBeNull();

    rerender(
      <MessageView
        message={{
          uid: 1,
          kind: "inbox",
          subject: "已打开的邮件",
          sender: { email: "sender@example.com", name: "Sender" },
          sent_at: "2026-07-21T12:00:00Z",
          body_fetched: false,
          preview: "正文预览",
        }}
        onClose={vi.fn()}
      />,
    );

    expect(screen.queryByTestId("reader-idle-experience")).toBeNull();
    expect(screen.getByText("已打开的邮件")).toBeTruthy();
  });

  it("uses the vertical-only scroll contract for an opened message", () => {
    const { container } = render(
      <MessageView
        message={{
          uid: 2,
          kind: "inbox",
          subject: "仅纵向滚动",
          sender: { email: "sender@example.com", name: "Sender" },
          sent_at: "2026-07-21T12:00:00Z",
          body_fetched: true,
          body_text: "正文",
        }}
        onClose={vi.fn()}
      />,
    );

    const scrollSurface = container.querySelector(".reader-scroll");
    expect(scrollSurface).toBeTruthy();
    expect(scrollSurface.classList.contains("vertical-scroll-surface")).toBe(true);
  });
});
