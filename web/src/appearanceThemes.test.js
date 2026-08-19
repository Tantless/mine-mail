import { afterEach, describe, expect, it } from "vitest";
import {
  appearancePalettes,
  appearanceFromSavedAppearance,
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
    expect(appearancePalettes).toHaveLength(28);
    expect(appearancePalettes.map((palette) => palette.id)).toEqual(
      expect.arrayContaining(["daylight", "night", "dusk", "forest"]),
    );
    expect(appearancePalettes.map((palette) => palette.id)).toContain(
      "sky-light",
    );
    expect(appearancePalettes.map((palette) => palette.id)).toContain(
      "sky-dark",
    );

    for (const palette of appearancePalettes) {
      expect(["light", "dark"]).toContain(palette.scheme);
      expect(palette.swatches).toHaveLength(4);
      expect(palette.tokens).toEqual(
        expect.objectContaining({
          canvas: expect.any(String),
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
    expect(normalizeAppearancePaletteId("unknown", "dark")).toBe("daylight");
  });

  it("preserves the four built-in themes as distinct complete palettes", () => {
    const palettes = Object.fromEntries(
      appearancePalettes.map((palette) => [palette.id, palette]),
    );
    expect(palettes.daylight.tokens.accent).toBe("#0878f9");
    expect(palettes.night.tokens.accent).toBe("#72aefc");
    expect(palettes.dusk.tokens.accent).toBe("#b95848");
    expect(palettes.forest.tokens.accent).toBe("#357a55");
    expect(
      new Set(
        ["daylight", "night", "dusk", "forest"].map(
          (id) => palettes[id].tokens.panel,
        ),
      ).size,
    ).toBe(4);
  });

  it("gives every palette a complete image-free minimal theme", () => {
    const root = document.documentElement;
    for (const palette of appearancePalettes) {
      applyAppearanceToDocument({
        ...defaultAppearance,
        selectionInitialized: true,
        paletteId: palette.id,
        minimalModeEnabled: true,
      });

      expect(root.dataset.theme).toBe("daylight");
      expect(root.dataset.appearanceMode).toBe("minimal");
      expect(root.dataset.paletteSource).toBe("selected");
      expect(root.style.getPropertyValue("--palette-color-scheme")).toBe(
        palette.scheme,
      );
      expect(root.style.getPropertyValue("--palette-canvas")).toBe(
        palette.tokens.canvas,
      );
      expect(root.style.getPropertyValue("--palette-panel")).toBe(
        palette.tokens.panel,
      );
      expect(root.style.getPropertyValue("--palette-text")).toBe(
        palette.tokens.text,
      );
      expect(root.style.getPropertyValue("--palette-danger")).toBe(
        palette.tokens.danger,
      );
    }
  });

  it("uses the preserved full theme style only for a matching original palette in image mode", () => {
    const root = document.documentElement;
    applyAppearanceToDocument({
      ...defaultAppearance,
      paletteId: "dusk",
      minimalModeEnabled: false,
      activeTheme: { kind: "builtin", id: "dusk" },
    });
    expect(root.dataset.paletteSource).toBe("theme-original");

    applyAppearanceToDocument({
      ...defaultAppearance,
      paletteId: "teal-light",
      minimalModeEnabled: false,
      activeTheme: { kind: "builtin", id: "dusk" },
    });
    expect(root.dataset.paletteSource).toBe("selected");
  });

  it("keeps a custom background dormant in minimal mode and restores saved preferences", () => {
    applyAppearanceToDocument({
      ...defaultAppearance,
      paletteId: "teal-dark",
      minimalModeEnabled: true,
      activeTheme: { kind: "custom", id: "preset-1" },
      customPresets: [
        { id: "preset-1", name: "海岸", focalX: 0.25, focalY: 0.75 },
      ],
      activeBackgroundDataUrl: "data:image/jpeg;base64,AQID",
    });

    const root = document.documentElement;
    expect(root.dataset.theme).toBe("custom");
    expect(root.dataset.appearanceMode).toBe("minimal");
    expect(root.style.getPropertyValue("--custom-wallpaper")).toContain("AQID");
    expect(appearanceFromSavedAppearance()).toEqual(
      expect.objectContaining({
        paletteId: "teal-dark",
        minimalModeEnabled: true,
      }),
    );
    expect(window.localStorage.getItem("mine-mail-custom-palette")).toBeNull();
  });
});
