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
    expect(screen.getByText("引用的原邮件")).toBeTruthy();
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
    expect(screen.queryByText("引用的原邮件")).toBeNull();
    expect(screen.getByText("Original body.")).toBeTruthy();
  });

  it("nests older quoted history behind one collapsible layer at a time", () => {
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
    expect(quotedLayers[0].contains(quotedLayers[1])).toBe(true);
    expect(quotedLayers[0].open).toBe(false);
    expect(quotedLayers[1].open).toBe(false);

    fireEvent.click(quotedLayers[0].querySelector(":scope > summary"));

    expect(quotedLayers[0].open).toBe(true);
    expect(quotedLayers[1].open).toBe(false);
    expect(screen.getByText("Older quoted body.")).toBeTruthy();
  });
});
