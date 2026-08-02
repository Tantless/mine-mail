import { describe, expect, it } from "vitest";
import {
  activeStepIndex,
  defaultPreviewInfo,
  stepTone,
} from "./installerState";

describe("installer state presentation", () => {
  it("keeps the step rail synchronized with real lifecycle states", () => {
    expect(activeStepIndex("ready")).toBe(0);
    expect(activeStepIndex("installing")).toBe(1);
    expect(activeStepIndex("success")).toBe(2);
    expect(activeStepIndex("error")).toBe(1);
  });

  it("marks completed and active steps without relying on static artwork", () => {
    expect(stepTone("installing", 0)).toBe("done");
    expect(stepTone("installing", 1)).toBe("active");
    expect(stepTone("installing", 2)).toBe("waiting");
    expect(stepTone("error", 1)).toBe("error");
  });

  it("provides a complete browser-preview fallback", () => {
    expect(defaultPreviewInfo()).toMatchObject({
      version: "1.0.0",
      payloadAvailable: true,
    });
  });
});
