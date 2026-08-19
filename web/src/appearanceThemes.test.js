import { afterEach, describe, expect, it } from "vitest";
import {
  appearancePalettes,
  appearanceFromSavedAppearance,
  applyAppearanceToDocument,
  defaultAppearance,
  normalizeAppearancePaletteId,
} from "./appearanceThemes.js";

function linearChannel(channel) {
  const value = channel / 255;
  return value <= 0.04045
    ? value / 12.92
    : ((value + 0.055) / 1.055) ** 2.4;
}

function linearRgb(hex) {
  return hex
    .slice(1)
    .match(/.{2}/g)
    .map((channel) => linearChannel(Number.parseInt(channel, 16)));
}

function relativeLuminance(hex) {
  const [red, green, blue] = linearRgb(hex);
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}

function contrastRatio(first, second) {
  const firstLuminance = relativeLuminance(first);
  const secondLuminance = relativeLuminance(second);
  return (
    (Math.max(firstLuminance, secondLuminance) + 0.05) /
    (Math.min(firstLuminance, secondLuminance) + 0.05)
  );
}

function oklch(hex) {
  const [red, green, blue] = linearRgb(hex);
  const l = Math.cbrt(
    0.4122214708 * red + 0.5363325363 * green + 0.0514459929 * blue,
  );
  const m = Math.cbrt(
    0.2119034982 * red + 0.6806995451 * green + 0.1073969566 * blue,
  );
  const s = Math.cbrt(
    0.0883024619 * red + 0.2817188376 * green + 0.6299787005 * blue,
  );
  const lightness = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const b = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  const hue = (Math.atan2(b, a) * 180) / Math.PI;
  return {
    lightness,
    chroma: Math.hypot(a, b),
    hue: hue < 0 ? hue + 360 : hue,
  };
}

function hueDifference(first, second) {
  const difference = Math.abs(first - second);
  return Math.min(difference, 360 - difference);
}

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
      expect(palette.minimalSwatches).toHaveLength(4);
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
      expect(palette.minimalTokens).toEqual(
        expect.objectContaining({
          canvas: expect.any(String),
          sidebar: expect.any(String),
          panel: expect.any(String),
          panelSubtle: expect.any(String),
          control: expect.any(String),
          text: expect.any(String),
          textSecondary: expect.any(String),
          selection: expect.any(String),
          selectionBorder: expect.any(String),
          onAccent: expect.any(String),
        }),
      );
    }
  });

  it("builds a restrained, palette-tinted minimal surface ladder", () => {
    const minimalCanvases = new Set();
    for (const palette of appearancePalettes) {
      const minimal = palette.minimalTokens;
      const seed = oklch(
        palette.scheme === "dark" ? palette.deep : palette.soft,
      );
      const canvas = oklch(minimal.canvas);
      const sidebar = oklch(minimal.sidebar);
      const panel = oklch(minimal.panel);
      const surfaces = [
        minimal.canvas,
        minimal.sidebar,
        minimal.panel,
        minimal.panelSubtle,
        minimal.control,
        minimal.selection,
      ];

      minimalCanvases.add(minimal.canvas);
      for (const surface of surfaces) {
        expect(surface, `${palette.id} should use opaque sRGB surfaces`).toMatch(
          /^#[0-9a-f]{6}$/,
        );
        expect(
          contrastRatio(minimal.text, surface),
          `${palette.id} primary text against ${surface}`,
        ).toBeGreaterThanOrEqual(7);
        expect(
          contrastRatio(minimal.textSecondary, surface),
          `${palette.id} secondary text against ${surface}`,
        ).toBeGreaterThanOrEqual(4.5);
        expect(
          contrastRatio(minimal.textMuted, surface),
          `${palette.id} muted text against ${surface}`,
        ).toBeGreaterThanOrEqual(4.5);
      }

      expect(
        contrastRatio(minimal.onAccent, palette.tokens.accent),
        `${palette.id} on-accent contrast`,
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrastRatio(minimal.selectionBorder, minimal.selection),
        `${palette.id} selection edge contrast`,
      ).toBeGreaterThanOrEqual(3);
      expect(Math.abs(sidebar.lightness - canvas.lightness)).toBeGreaterThan(
        0.025,
      );
      expect(Math.abs(sidebar.lightness - canvas.lightness)).toBeLessThan(0.06);
      expect(Math.abs(panel.lightness - canvas.lightness)).toBeGreaterThan(0.025);
      expect(Math.abs(panel.lightness - canvas.lightness)).toBeLessThan(0.06);
      expect(hueDifference(canvas.hue, seed.hue)).toBeLessThan(5);

      if (palette.scheme === "light") {
        expect(canvas.lightness).toBeGreaterThan(0.87);
        expect(canvas.lightness).toBeLessThan(0.89);
      } else {
        expect(canvas.lightness).toBeGreaterThan(0.29);
        expect(canvas.lightness).toBeLessThan(0.31);
      }
    }
    expect(minimalCanvases.size).toBe(appearancePalettes.length);
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
    expect(
      new Set(
        ["daylight", "night", "dusk", "forest"].map(
          (id) => palettes[id].minimalTokens.canvas,
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
      expect(root.style.getPropertyValue("--palette-minimal-canvas")).toBe(
        palette.minimalTokens.canvas,
      );
      expect(root.style.getPropertyValue("--palette-minimal-sidebar")).toBe(
        palette.minimalTokens.sidebar,
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
