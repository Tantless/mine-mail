import { afterEach, describe, expect, it } from "vitest";
import {
  appearancePalettes,
  applyAppearanceToDocument,
  defaultAppearance,
  normalizeAppearancePaletteId,
} from "./appearanceThemes.js";

describe("appearance semantic palettes", () => {
  afterEach(() => {
    applyAppearanceToDocument(defaultAppearance);
    window.localStorage.clear();
  });

  it("provides complete light and dark semantic palettes for every color family", () => {
    expect(appearancePalettes).toHaveLength(24);
    expect(appearancePalettes.map((palette) => palette.id)).toContain("sky-light");
    expect(appearancePalettes.map((palette) => palette.id)).toContain("sky-dark");

    for (const palette of appearancePalettes) {
      expect(["light", "dark"]).toContain(palette.scheme);
      expect(palette.swatches).toHaveLength(4);
      expect(palette.tokens).toEqual(
        expect.objectContaining({
          glass: expect.any(String),
          panel: expect.any(String),
          text: expect.any(String),
          border: expect.any(String),
          accent: expect.any(String),
          selection: expect.any(String),
          success: expect.any(String),
          warning: expect.any(String),
          danger: expect.any(String),
        }),
      );
    }
  });

  it("folds legacy palette and mode values into one complete palette id", () => {
    expect(normalizeAppearancePaletteId("teal", "dark")).toBe("teal-dark");
    expect(normalizeAppearancePaletteId("rose", "light")).toBe("rose-light");
    expect(normalizeAppearancePaletteId("sky-dark", "light")).toBe("sky-dark");
    expect(normalizeAppearancePaletteId("unknown", "dark")).toBe("sky-light");
  });

  it("applies glass, content, interaction, and state tones from one palette", () => {
    const palette = appearancePalettes.find((item) => item.id === "teal-dark");
    applyAppearanceToDocument({
      selectionInitialized: true,
      activeTheme: { kind: "custom", id: "preset-1" },
      customPresets: [
        {
          id: "preset-1",
          name: "海岸",
          paletteId: palette.id,
          focalX: 0.25,
          focalY: 0.75,
        },
      ],
      activeBackgroundDataUrl: "data:image/jpeg;base64,AQID",
    });

    const root = document.documentElement;
    expect(root.dataset.theme).toBe("custom");
    expect(root.dataset.colorMode).toBeUndefined();
    expect(root.style.getPropertyValue("--custom-color-scheme")).toBe("dark");
    expect(root.style.getPropertyValue("--custom-glass")).toBe(palette.tokens.glass);
    expect(root.style.getPropertyValue("--custom-panel")).toBe(palette.tokens.panel);
    expect(root.style.getPropertyValue("--custom-text")).toBe(palette.tokens.text);
    expect(root.style.getPropertyValue("--custom-danger")).toBe(palette.tokens.danger);
    expect(window.localStorage.getItem("mine-mail-custom-palette")).toBe("teal-dark");
    expect(window.localStorage.getItem("mine-mail-custom-mode")).toBeNull();
  });
});
