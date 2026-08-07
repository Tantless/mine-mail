import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  HtmlMessageBody,
  buildEmailDocument,
  fitEmailDocumentToWidth,
} from "./HtmlMessageBody.jsx";

function setLayoutMetric(node, property, value) {
  Object.defineProperty(node, property, {
    configurable: true,
    get: typeof value === "function" ? value : () => value,
  });
}

describe("HTML message body", () => {
  afterEach(() => cleanup());

  it("builds a no-script document that blocks remote images by default", () => {
    const document = buildEmailDocument('<img src="https://images.example/logo.png">', false);

    expect(document).toContain("script-src 'none'");
    expect(document).toContain("img-src data: blob:");
    expect(document).not.toContain("img-src data: blob: http: https:");
    expect(document).toContain('img[src^="https://"]');
    expect(document).toContain("overflow: hidden !important");
    expect(document).toContain("pre {");
    expect(document).toContain("overflow: visible");
  });

  it("keeps content-sized iframe media queries on a stable portrait branch", () => {
    const document = buildEmailDocument(`
      <style>
        @media screen and (max-width: 767px) and (orientation: landscape) {
          .card { width: 360px; }
        }
        @media screen and (orientation: portrait) {
          .card { width: 568px; }
        }
      </style>
      <p>(orientation: landscape)</p>
    `);

    expect(document).toContain(
      "and (min-width: 100000px)",
    );
    expect(document).toContain("and (min-width: 0px)");
    expect(document).not.toContain("and (orientation: landscape)");
    expect(document).not.toContain("and (orientation: portrait)");
    expect(document).toContain("<p>(orientation: landscape)</p>");
  });

  it("repairs the line rhythm of legacy Mine Mail stationery", () => {
    const document = buildEmailDocument(
      '<div data-mine-mail-stationery="lined"><p>第一行</p><p>第二行</p></div>',
      false,
    );

    expect(document).toContain(
      '[data-mine-mail-stationery="lined"] p',
    );
    expect(document).toContain(
      '[data-mine-mail-stationery="grid"] p',
    );
    expect(document).toContain("margin: 0 !important");
    expect(document).toContain("line-height: 28px !important");
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
    expect(screen.getByLabelText("正在加载正文")).toBeTruthy();
    expect(frame.dataset.ready).toBe("false");
    expect(frame.getAttribute("sandbox")).toBe("allow-same-origin");
    expect(frame.getAttribute("scrolling")).toBe("no");
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

  it("scales a clipped nested table even when the document root reports no overflow", () => {
    const document = new DOMParser().parseFromString(
      `<table class="responsive" width="740" style="width:740px">
        <tbody><tr><td><table width="620"><tbody><tr>
          <td data-title>Wide PlayStation poster title</td>
        </tr></tbody></table></td></tr></tbody>
      </table>`,
      "text/html",
    );
    const title = document.querySelector("[data-title]");
    setLayoutMetric(document.documentElement, "scrollWidth", 620);
    setLayoutMetric(document.documentElement, "scrollHeight", 1_000);
    setLayoutMetric(document.body, "scrollWidth", 620);
    setLayoutMetric(document.body, "scrollHeight", 1_000);
    setLayoutMetric(document.body, "offsetHeight", 1_000);
    setLayoutMetric(title, "clientWidth", 540);
    setLayoutMetric(title, "scrollWidth", 620);
    title.getBoundingClientRect = () => ({
      bottom: 100,
      height: 100,
      left: 60,
      right: 600,
      top: 0,
      width: 540,
      x: 60,
      y: 0,
      toJSON: () => ({}),
    });

    const result = fitEmailDocumentToWidth(document, 620);

    expect(result.scale).toBeCloseTo(620 / 740);
    expect(result.height).toBe(838);
    expect(document.body.style.getPropertyValue("width")).toBe("740px");
    expect(document.body.style.getPropertyPriority("width")).toBe("important");
    expect(document.body.style.getPropertyValue("transform")).toContain("scale(");
    expect(document.body.dataset.mineMailWidthFit).toBe("true");
  });

  it("does not scale a declared desktop width when responsive content is not clipped", () => {
    const document = new DOMParser().parseFromString(
      '<table class="responsive" width="740"><tbody><tr><td>Responsive</td></tr></tbody></table>',
      "text/html",
    );
    setLayoutMetric(document.documentElement, "scrollWidth", 620);
    setLayoutMetric(document.documentElement, "scrollHeight", 1_000);
    setLayoutMetric(document.body, "scrollWidth", 620);
    setLayoutMetric(document.body, "scrollHeight", 1_000);
    setLayoutMetric(document.body, "offsetHeight", 1_000);

    const result = fitEmailDocumentToWidth(document, 620);

    expect(result).toEqual({ height: 1_000, scale: 1 });
    expect(document.body.style.getPropertyValue("width")).toBe("");
    expect(document.body.dataset.mineMailWidthFit).toBeUndefined();
  });

  it("shrinks sent stationery below the loading placeholder height", () => {
    const document = new DOMParser().parseFromString(
      '<div data-mine-mail-stationery="lined" style="box-sizing:border-box;min-height:168px">正文</div>',
      "text/html",
    );
    setLayoutMetric(document.documentElement, "clientHeight", 220);
    setLayoutMetric(document.documentElement, "scrollWidth", 620);
    setLayoutMetric(document.documentElement, "scrollHeight", 220);
    setLayoutMetric(document.body, "scrollWidth", 620);
    setLayoutMetric(document.body, "scrollHeight", 168);
    setLayoutMetric(document.body, "offsetHeight", 168);

    expect(fitEmailDocumentToWidth(document, 620)).toEqual({
      height: 168,
      scale: 1,
    });
  });

  it("leaves documents unchanged when the reader is wide enough", () => {
    const document = new DOMParser().parseFromString(
      '<table width="740"><tbody><tr><td>Poster</td></tr></tbody></table>',
      "text/html",
    );
    let layoutWidth = 740;
    setLayoutMetric(document.documentElement, "scrollWidth", () => layoutWidth);
    setLayoutMetric(document.documentElement, "scrollHeight", 1_000);
    setLayoutMetric(document.body, "scrollWidth", () => layoutWidth);
    setLayoutMetric(document.body, "scrollHeight", 1_000);
    setLayoutMetric(document.body, "offsetHeight", 1_000);

    fitEmailDocumentToWidth(document, 620);
    layoutWidth = 800;
    const result = fitEmailDocumentToWidth(document, 800);

    expect(result).toEqual({ height: 1_000, scale: 1 });
    expect(document.body.style.getPropertyValue("width")).toBe("");
    expect(document.body.style.getPropertyValue("transform")).toBe("");
    expect(document.body.dataset.mineMailWidthFit).toBeUndefined();
  });
});
