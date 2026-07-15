import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { HtmlMessageBody, buildEmailDocument } from "./HtmlMessageBody.jsx";

describe("HTML message body", () => {
  afterEach(() => cleanup());

  it("builds a no-script document that blocks remote images by default", () => {
    const document = buildEmailDocument('<img src="https://images.example/logo.png">', false);

    expect(document).toContain("script-src 'none'");
    expect(document).toContain("img-src data: blob:");
    expect(document).not.toContain("img-src data: blob: http: https:");
    expect(document).toContain('img[src^="https://"]');
  });

  it("loads remote images automatically by default", () => {
    render(
      <HtmlMessageBody
        html={'<img src="https://images.example/logo.png">'}
        hasRemoteImages
        title="Automatic remote mail"
        onOpenLink={vi.fn()}
      />,
    );

    const frame = screen.getByTitle("Automatic remote mail HTML 正文");
    expect(frame.getAttribute("sandbox")).toBe("allow-same-origin");
    expect(frame.getAttribute("srcdoc")).toContain(
      "img-src data: blob: http: https:",
    );
    expect(screen.queryByRole("button", { name: "加载远程图片" })).toBeNull();
  });

  it("only enables remote images after an explicit click in ask mode", async () => {
    const user = userEvent.setup();
    render(
      <HtmlMessageBody
        html={'<img src="https://images.example/logo.png">'}
        hasRemoteImages
        remoteImageMode="ask"
        title="Remote mail"
        onOpenLink={vi.fn()}
      />,
    );

    const frame = screen.getByTitle("Remote mail HTML 正文");
    expect(frame.getAttribute("sandbox")).toBe("allow-same-origin");
    expect(frame.getAttribute("srcdoc")).toContain("img-src data: blob:");

    await user.click(screen.getByRole("button", { name: "加载远程图片" }));
    expect(frame.getAttribute("srcdoc")).toContain(
      "img-src data: blob: http: https:",
    );
  });

  it("keeps remote images blocked without offering a one-time override", () => {
    render(
      <HtmlMessageBody
        html={'<img src="https://images.example/logo.png">'}
        hasRemoteImages
        remoteImageMode="blocked"
        title="Blocked remote mail"
        onOpenLink={vi.fn()}
      />,
    );

    const frame = screen.getByTitle("Blocked remote mail HTML 正文");
    expect(frame.getAttribute("srcdoc")).toContain("img-src data: blob:");
    expect(screen.getByText("已根据设置阻止远程图片")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "加载远程图片" })).toBeNull();
  });
});
