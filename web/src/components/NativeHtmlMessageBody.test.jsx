import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  NativeHtmlMessageBody,
  prepareNativeHtml,
} from "./NativeHtmlMessageBody.jsx";

describe("native HTML message body", () => {
  afterEach(() => cleanup());

  it("keeps semantic markup on the themed transparent reader surface", () => {
    const { container } = render(
      <NativeHtmlMessageBody
        html={'<p>Hello <strong>Myo</strong></p><blockquote>Reply</blockquote>'}
      />,
    );

    expect(screen.getByText("Myo").tagName).toBe("STRONG");
    expect(screen.getByText("Reply").tagName).toBe("BLOCKQUOTE");
    expect(container.querySelector("iframe")).toBeNull();
    expect(container.querySelector(".native-html-message__content")).toBeTruthy();
  });

  it("removes remote sources before insertion when images are not allowed", () => {
    const html = prepareNativeHtml(
      '<img alt="remote" src="https://images.example/avatar.png"><img alt="inline" src="data:image/png;base64,AQID">',
      false,
    );
    const template = document.createElement("template");
    template.innerHTML = html;
    const [remote, inline] = template.content.querySelectorAll("img");

    expect(remote.hasAttribute("src")).toBe(false);
    expect(remote.dataset.mineMailImageBlocked).toBe("true");
    expect(inline.getAttribute("src")).toBe("data:image/png;base64,AQID");
  });

  it("loads remote images automatically by default", () => {
    const { container } = render(
      <NativeHtmlMessageBody
        html={'<img alt="remote" src="https://images.example/avatar.png">'}
        hasRemoteImages
      />,
    );

    expect(container.querySelector("img").getAttribute("src")).toBe(
      "https://images.example/avatar.png",
    );
    expect(screen.queryByRole("button", { name: "加载远程图片" })).toBeNull();
  });

  it("only inserts a remote source after approval in ask mode", async () => {
    const user = userEvent.setup();
    const { container } = render(
      <NativeHtmlMessageBody
        html={'<img alt="remote" src="https://images.example/avatar.png">'}
        hasRemoteImages
        remoteImageMode="ask"
      />,
    );

    expect(container.querySelector("img").hasAttribute("src")).toBe(false);
    await user.click(screen.getByRole("button", { name: "加载远程图片" }));
    expect(container.querySelector("img").getAttribute("src")).toBe(
      "https://images.example/avatar.png",
    );
  });

  it("routes links through the desktop external-link boundary", () => {
    const onOpenLink = vi.fn();
    render(
      <NativeHtmlMessageBody
        html={'<a href="https://paa.moe">Open</a>'}
        onOpenLink={onOpenLink}
      />,
    );

    fireEvent.click(screen.getByRole("link", { name: "Open" }));
    expect(onOpenLink).toHaveBeenCalledWith("https://paa.moe");
  });
});
