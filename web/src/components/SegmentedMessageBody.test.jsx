import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { SegmentedMessageBody } from "./SegmentedMessageBody.jsx";

const message = {
  mailbox: "INBOX",
  uid: 42,
  subject: "Reply",
  body_text: "My reply.\n\nOriginal body.",
  body_html: null,
  body_segments: [
    {
      kind: "authored",
      content: "My reply.",
      render_mode: "plain",
      quote_depth: 0,
      confidence: "high",
    },
    {
      kind: "quoted",
      content: "Original body.",
      render_mode: "plain",
      quote_depth: 1,
      confidence: "high",
      quote_metadata: {
        subject: "Earlier note",
        sender: "sender@example.com",
        recipient: "receiver@example.com",
        sent_at: "2026-07-01 20:15",
      },
    },
  ],
};

describe("segmented message body", () => {
  afterEach(() => cleanup());

  it("keeps authored text native and collapses high-confidence history", () => {
    const { container } = render(
      <SegmentedMessageBody
        message={message}
        body={message.body_text}
        bodyRenderMode="plain"
      />,
    );

    expect(screen.getByText("My reply.")).toBeTruthy();
    expect(screen.getByText("Earlier note")).toBeTruthy();
    expect(screen.getByText("sender@example.com")).toBeTruthy();
    expect(screen.getByText("receiver@example.com")).toBeTruthy();
    expect(screen.getByText("2026-07-01 20:15")).toBeTruthy();
    expect(container.querySelector("details").open).toBe(false);
    expect(container.querySelector("iframe")).toBeNull();
  });

  it("offers an explicit original-format fallback", () => {
    render(
      <SegmentedMessageBody
        message={message}
        body={message.body_text}
        bodyRenderMode="plain"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "按原始格式查看" }));

    expect(screen.getByRole("button", { name: "返回分段阅读" })).toBeTruthy();
    expect(screen.queryByText("Earlier note")).toBeNull();
    expect(screen.getByText("Original body.")).toBeTruthy();
  });

  it("keeps NetEase At-wrote metadata out of the transparent default reader", () => {
    const neteaseMessage = {
      ...message,
      subject: "Re:1",
      body_text:
        '123\n\nAt 2026-07-17 09:54:29, "tantless" <sender@example.com> wrote:\n\nOriginal body',
      body_html:
        '<div style="background:#aaa">123</div><p>At 2026-07-17 09:54:29, tantless wrote:</p><blockquote>Original body</blockquote>',
      body_render_mode: "isolated_html",
      body_segments: [
        {
          kind: "authored",
          content: "123",
          render_mode: "plain",
          quote_depth: 0,
          confidence: "high",
        },
        {
          kind: "quoted",
          content:
            '<div>Original <a href="https://paa.moe">linked body</a></div><img alt="Myo avatar" src="data:image/png;base64,AQID">',
          render_mode: "native_html",
          quote_depth: 1,
          confidence: "high",
          quote_metadata: {
            subject: "1",
            sender: "tantless <sender@example.com>",
            recipient: "Mine Mail <receiver@example.com>",
            sent_at: "2026-07-17T09:54:29+08:00",
          },
        },
      ],
    };
    const { container } = render(
      <SegmentedMessageBody
        message={neteaseMessage}
        body={neteaseMessage.body_text}
        bodyRenderMode={neteaseMessage.body_render_mode}
      />,
    );

    expect(screen.getByText("123")).toBeTruthy();
    expect(screen.getByText("1")).toBeTruthy();
    expect(screen.getByText("tantless <sender@example.com>")).toBeTruthy();
    expect(screen.getByText("Mine Mail <receiver@example.com>")).toBeTruthy();
    expect(screen.queryByText(/At 2026-07-17/)).toBeNull();
    expect(container.querySelector("iframe")).toBeNull();
    fireEvent.click(screen.getByText("1").closest("summary"));
    expect(screen.getByRole("link", { name: "linked body" })).toBeTruthy();
    expect(screen.getByAltText("Myo avatar")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "按原始格式查看" }));
    expect(container.querySelector("iframe")).toBeTruthy();
  });

  it("shows every quoted message as an independent top-level collapsible card", () => {
    const nestedMessage = {
      ...message,
      body_segments: [
        message.body_segments[0],
        message.body_segments[1],
        {
          kind: "quoted",
          content: "Older quoted body.",
          render_mode: "plain",
          quote_depth: 2,
          confidence: "high",
          quote_metadata: {
            subject: "Oldest note",
            sender: "older@example.com",
            recipient: "sender@example.com",
            sent_at: "2026-06-30 09:00",
          },
        },
      ],
    };
    const { container } = render(
      <SegmentedMessageBody
        message={nestedMessage}
        body={nestedMessage.body_text}
        bodyRenderMode="plain"
      />,
    );

    const quotedLayers = container.querySelectorAll("details.quoted-message");
    expect(quotedLayers).toHaveLength(2);
    expect(quotedLayers[0].contains(quotedLayers[1])).toBe(false);
    expect(quotedLayers[0].parentElement).toBe(quotedLayers[1].parentElement);
    expect(screen.getByText("Earlier note")).toBeTruthy();
    expect(screen.getByText("Oldest note")).toBeTruthy();
    expect(quotedLayers[0].open).toBe(false);
    expect(quotedLayers[1].open).toBe(false);

    fireEvent.click(quotedLayers[0].querySelector(":scope > summary"));

    expect(quotedLayers[0].open).toBe(true);
    expect(quotedLayers[1].open).toBe(false);
  });
});
