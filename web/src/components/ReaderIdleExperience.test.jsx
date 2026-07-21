import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  READER_IDLE_SCENE_DURATION_MS,
  READER_IDLE_SCENES,
  ReaderIdleExperience,
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
    vi.spyOn(window.navigator, "userAgent", "get").mockReturnValue(
      "Mine Mail test browser",
    );
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

  it("renders every scene as a staggered sequence of individual characters", () => {
    const { container } = render(<ReaderIdleExperience />);
    const idle = container.querySelector(".reader-idle");
    const characters = Array.from(
      container.querySelectorAll(".reader-idle__char"),
    );

    expect(READER_IDLE_SCENES).toHaveLength(4);
    expect(idle.getAttribute("aria-hidden")).toBe("true");
    expect(idle.dataset.scene).toBe("0");
    expect(
      container.querySelector(".reader-idle__figure").dataset.effect,
    ).toBe("char-rise");
    expect(container.textContent).toContain("海内存知己，天涯若比邻。");
    expect(container.textContent).toContain("——王勃 · 送杜少府之任蜀州");
    expect(
      container.querySelector(".reader-idle__quote").dataset.staggered,
    ).toBe("true");
    expect(
      container
        .querySelector(".reader-idle__figure")
        .style.getPropertyValue("--reader-idle-composition-units"),
    ).toBe("10.48");
    expect(characters).toHaveLength(Array.from("海内存知己，天涯若比邻。").length);
    expect(characters[0].style.getPropertyValue("--char-index")).toBe("0");
    expect(characters.at(-1).style.getPropertyValue("--char-index")).toBe(
      String(characters.length - 1),
    );
    expect(container.querySelector("canvas")).toBeNull();
    expect(container.querySelector(".reader-idle__glow")).toBeNull();
    expect(container.querySelector(".reader-idle__signal")).toBeNull();

    act(() => vi.advanceTimersByTime(READER_IDLE_SCENE_DURATION_MS));

    expect(idle.dataset.scene).toBe("1");
    expect(
      container.querySelector(".reader-idle__figure").dataset.effect,
    ).toBe("char-fly");
    expect(container.textContent).toContain("云中谁寄锦书来？");
    expect(
      container.querySelector(".reader-idle__quote").dataset.staggered,
    ).toBeUndefined();
    expect(
      container
        .querySelector(".reader-idle__figure")
        .style.getPropertyValue("--reader-idle-composition-units"),
    ).toBe("8.64");
  });

  it("pauses scene rotation while the document is hidden", () => {
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

  it("fixes the first scene when reduced motion is preferred", () => {
    const preference = installMotionPreference(true);
    const { container } = render(<ReaderIdleExperience />);
    const idle = container.querySelector(".reader-idle");

    act(() => vi.advanceTimersByTime(READER_IDLE_SCENE_DURATION_MS * 3));

    expect(idle.dataset.scene).toBe("0");
    expect(idle.dataset.reducedMotion).toBe("true");

    act(() => preference.setMatches(false));
    act(() => vi.advanceTimersByTime(READER_IDLE_SCENE_DURATION_MS));
    expect(idle.dataset.scene).toBe("1");
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
