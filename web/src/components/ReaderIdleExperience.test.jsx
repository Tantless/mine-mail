import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  READER_IDLE_SCENE_DURATION_MS,
  READER_IDLE_SCENES,
  ReaderIdleExperience,
  chooseRandomSceneIndex,
  layoutQuoteText,
} from "./ReaderIdleExperience.jsx";

function installMotionPreference(matches = false) {
  const listeners = new Set();
  const mediaQuery = {
    matches,
    media: "(prefers-reduced-motion: reduce)",
    addEventListener: vi.fn((_event, listener) => listeners.add(listener)),
    removeEventListener: vi.fn((_event, listener) => listeners.delete(listener)),
  };
  vi.stubGlobal("matchMedia", vi.fn(() => mediaQuery));
  return {
    mediaQuery,
    setMatches(nextMatches) {
      mediaQuery.matches = nextMatches;
      for (const listener of listeners) listener({ matches: nextMatches });
    },
  };
}

describe("reader idle experience", () => {
  let hidden = false;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(Math, "random").mockReturnValue(0);
    hidden = false;
    Object.defineProperty(document, "hidden", {
      configurable: true,
      get: () => hidden,
    });
    installMotionPreference(false);
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("uses only the supplied 42-entry quote library", () => {
    expect(READER_IDLE_SCENES).toHaveLength(42);
    expect(READER_IDLE_SCENES[0]).toEqual({
      id: 1,
      text: "我选择了人迹更少的一条路，而这改变了一切。",
      source: "罗伯特·弗罗斯特：《未选择的路》",
    });
    expect(READER_IDLE_SCENES.at(-1)).toEqual({
      id: 42,
      text: "拣尽寒枝不肯栖，寂寞沙洲冷。",
      source: "苏轼：《卜算子·黄州定慧院寓居作》",
    });
    expect(READER_IDLE_SCENES.some((scene) => scene.source === "MINE MAIL")).toBe(
      false,
    );
  });

  it("keeps natural couplets balanced and anchors unequal lines without cropping", () => {
    const couplet = layoutQuoteText(READER_IDLE_SCENES[24].text);
    const prose = layoutQuoteText(READER_IDLE_SCENES[0].text);

    expect(couplet).toMatchObject({
      layout: "balanced",
      lines: ["海内存知己，", "天涯若比邻。"],
      starts: [0, 4],
    });
    expect(prose.layout).toBe("flow");
    expect(prose.lines.join("")).toBe(READER_IDLE_SCENES[0].text);
    expect(Math.max(...prose.lines.map((line) => Array.from(line).length))).toBeLessThanOrEqual(
      14,
    );
  });

  it("fits all 42 quotations in one-line rows using first, middle, and last anchors", () => {
    for (const scene of READER_IDLE_SCENES) {
      const layout = layoutQuoteText(scene.text);
      const widths = layout.lines.map(
        (line) => Array.from(line).length * 1.08,
      );

      expect(layout.lines.join(""), `quote ${scene.id}`).toBe(scene.text);
      expect(
        Math.max(...layout.lines.map((line) => Array.from(line).length)),
        `quote ${scene.id}`,
      ).toBeLessThanOrEqual(14);
      expect(layout.starts[0]).toBe(0);

      layout.starts.forEach((start, index) => {
        expect(
          start + widths[index],
          `quote ${scene.id}, line ${index + 1}`,
        ).toBeLessThanOrEqual(layout.frameUnits + 0.0001);
      });

      if (layout.layout === "flow" && layout.lines.length > 1) {
        const lastStart = layout.starts.at(-1);
        expect(lastStart + widths.at(-1)).toBeCloseTo(layout.frameUnits, 5);
        for (let index = 1; index < layout.lines.length - 1; index += 1) {
          expect(layout.starts[index]).toBeCloseTo(
            lastStart * (index / (layout.lines.length - 1)),
            5,
          );
        }
      }
    }
  });

  it("renders characters individually with a continuous source dash and no debug control", () => {
    const { container } = render(<ReaderIdleExperience />);
    const idle = container.querySelector(".reader-idle");
    const figure = container.querySelector(".reader-idle__figure");
    const characters = Array.from(
      container.querySelectorAll(".reader-idle__char"),
    );

    expect(idle.getAttribute("aria-hidden")).toBeNull();
    expect(figure.getAttribute("aria-hidden")).toBe("true");
    expect(idle.dataset.scene).toBe("0");
    expect(figure.dataset.effect).toBe("char-rise");
    expect(figure.style.getPropertyValue("--reader-idle-frame-units")).toBeTruthy();
    expect(container.textContent).toContain(READER_IDLE_SCENES[0].text);
    expect(container.textContent).toContain(READER_IDLE_SCENES[0].source);
    expect(container.textContent).not.toContain("——");
    expect(container.querySelector(".reader-idle__source-dash")).toBeTruthy();
    expect(characters).toHaveLength(Array.from(READER_IDLE_SCENES[0].text).length);
    expect(characters[0].style.getPropertyValue("--char-index")).toBe("0");
    expect(
      container
        .querySelector(".reader-idle__line")
        .style.getPropertyValue("--reader-idle-line-start"),
    ).toBe("0em");
    expect(characters.at(-1).style.getPropertyValue("--char-index")).toBe(
      String(characters.length - 1),
    );
    expect(container.querySelector(".reader-idle__review-control")).toBeNull();
  });

  it("plays randomly without an immediate repeat", () => {
    const { container } = render(<ReaderIdleExperience />);
    const idle = container.querySelector(".reader-idle");

    expect(chooseRandomSceneIndex(0, () => 0)).toBe(1);
    act(() => vi.advanceTimersByTime(READER_IDLE_SCENE_DURATION_MS));
    expect(idle.dataset.scene).toBe("1");
    expect(container.textContent).toContain(READER_IDLE_SCENES[1].text);

    act(() => vi.advanceTimersByTime(READER_IDLE_SCENE_DURATION_MS));
    expect(idle.dataset.scene).toBe("0");
    expect(container.textContent).toContain(READER_IDLE_SCENES[0].text);
  });

  it("pauses random rotation while the document is hidden", () => {
    const { container } = render(<ReaderIdleExperience />);
    const idle = container.querySelector(".reader-idle");

    hidden = true;
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    act(() => vi.advanceTimersByTime(READER_IDLE_SCENE_DURATION_MS * 2));

    expect(idle.dataset.scene).toBe("0");
    expect(idle.dataset.paused).toBe("true");

    hidden = false;
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    act(() => vi.advanceTimersByTime(READER_IDLE_SCENE_DURATION_MS));

    expect(idle.dataset.scene).toBe("1");
    expect(idle.dataset.paused).toBeUndefined();
  });

  it("disables automatic rotation for reduced motion", () => {
    installMotionPreference(true);
    const { container } = render(<ReaderIdleExperience />);
    const idle = container.querySelector(".reader-idle");

    act(() => vi.advanceTimersByTime(READER_IDLE_SCENE_DURATION_MS * 3));
    expect(idle.dataset.scene).toBe("0");
    expect(idle.dataset.reducedMotion).toBe("true");

    expect(container.querySelector("button")).toBeNull();
  });

  it("clears timers and event listeners when unmounted", () => {
    const preference = installMotionPreference(false);
    const clearTimeoutSpy = vi.spyOn(window, "clearTimeout");
    const removeVisibilitySpy = vi.spyOn(document, "removeEventListener");

    const { unmount } = render(<ReaderIdleExperience />);
    unmount();

    expect(clearTimeoutSpy).toHaveBeenCalled();
    expect(preference.mediaQuery.removeEventListener).toHaveBeenCalledWith(
      "change",
      expect.any(Function),
    );
    expect(removeVisibilitySpy).toHaveBeenCalledWith(
      "visibilitychange",
      expect.any(Function),
    );
  });
});
