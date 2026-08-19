import { describe, expect, it } from "vitest";
import {
  limitText,
  textCharacterCount,
  textInputLimits,
} from "./textLimits.js";

describe("text limits", () => {
  it("counts a Unicode code point once and truncates without splitting it", () => {
    expect(textCharacterCount("邮📎件")).toBe(3);
    expect(limitText("邮📎件", 2)).toBe("邮📎");
  });

  it("keeps values that are already within their limit unchanged", () => {
    const value = "正文".repeat(textInputLimits.composeBody / 2);
    expect(limitText(value, textInputLimits.composeBody)).toBe(value);
  });
});
