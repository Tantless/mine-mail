import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

describe("notification sound playback", () => {
  let audioInstances;

  beforeEach(() => {
    vi.resetModules();
    audioInstances = [];
    vi.stubGlobal(
      "Audio",
      class FakeAudio {
        constructor(src) {
          this.src = src;
          this.preload = "none";
          this.currentTime = 10;
          this.play = vi.fn().mockResolvedValue(undefined);
          audioInstances.push(this);
        }
      },
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("loads the selected bundled asset and reuses its audio element", async () => {
    const { playWebNotificationSound } = await import("./notificationSound.js");

    await playWebNotificationSound("double_chime");
    audioInstances[0].currentTime = 4;
    await playWebNotificationSound("double_chime");

    expect(audioInstances).toHaveLength(1);
    expect(audioInstances[0].src).toBe(
      "/sounds/notifications/double-chime.wav",
    );
    expect(audioInstances[0].preload).toBe("auto");
    expect(audioInstances[0].currentTime).toBe(0);
    expect(audioInstances[0].play).toHaveBeenCalledTimes(2);
  });

  it("falls back to the minimal preset for an unknown value", async () => {
    const { playWebNotificationSound } = await import("./notificationSound.js");

    await playWebNotificationSound("obsolete");

    expect(audioInstances[0].src).toBe("/sounds/notifications/minimal.wav");
  });

  it("surfaces playback failures to the preview caller", async () => {
    const playbackFailure = new Error("playback failed");
    vi.stubGlobal(
      "Audio",
      class FailingAudio {
        constructor() {
          this.play = vi.fn().mockRejectedValue(playbackFailure);
        }
      },
    );
    const { playWebNotificationSound } = await import("./notificationSound.js");

    await expect(playWebNotificationSound("bubble")).rejects.toBe(
      playbackFailure,
    );
  });
});
