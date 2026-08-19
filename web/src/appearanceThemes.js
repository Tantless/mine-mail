const paletteFamilies = Object.freeze([
  {
    id: "green",
    name: "青绿",
    main: "#79b96a",
    soft: "#b8e89d",
    deep: "#4e7f43",
  },
  {
    id: "teal",
    name: "薄荷",
    main: "#66d3b6",
    soft: "#9cebd6",
    deep: "#198f76",
  },
  {
    id: "cyan",
    name: "湖蓝",
    main: "#67c7d8",
    soft: "#9ae2eb",
    deep: "#338b9d",
  },
  {
    id: "sky",
    name: "晴蓝",
    main: "#78b9ee",
    soft: "#b5dcfa",
    deep: "#3e7fb4",
  },
  {
    id: "blue",
    name: "群青",
    main: "#858fe7",
    soft: "#c1c8fa",
    deep: "#545eae",
  },
  {
    id: "indigo",
    name: "靛青",
    main: "#9c7fdf",
    soft: "#d1bdf6",
    deep: "#6b4cac",
  },
  {
    id: "violet",
    name: "紫罗兰",
    main: "#b37bd8",
    soft: "#dfb9f0",
    deep: "#7b49a0",
  },
  {
    id: "purple",
    name: "木槿",
    main: "#cc7ed5",
    soft: "#edb9ee",
    deep: "#934a9b",
  },
  {
    id: "magenta",
    name: "莓粉",
    main: "#df82b0",
    soft: "#f5bdd7",
    deep: "#a64d78",
  },
  {
    id: "rose",
    name: "珊瑚",
    main: "#e98c7a",
    soft: "#ffc3b5",
    deep: "#b15549",
  },
  {
    id: "orange",
    name: "暖橙",
    main: "#e9a24f",
    soft: "#ffd19a",
    deep: "#ad6c25",
  },
  {
    id: "yellow",
    name: "麦黄",
    main: "#d4c34f",
    soft: "#f3e781",
    deep: "#948529",
  },
]);

const semanticByScheme = Object.freeze({
  light: {
    schemeLabel: "明亮",
    textBase: "#172027",
    textSecondaryBase: "#445159",
    textMutedBase: "#748087",
    surfaceBase: "#f9fbfa",
    surfaceSubtleBase: "#f1f5f3",
    controlBase: "#edf3f1",
    borderBase: "#d5dfdc",
    dividerBase: "#dde5e2",
    glassBase: "#edf4f1",
    overlay: "rgba(12, 20, 24, 0.38)",
    sidebarText: "#f5f8f7",
    sidebarMuted: "rgba(239, 246, 243, 0.76)",
    success: "#2f8a59",
    warning: "#a66a18",
    danger: "#c94a4a",
    favorite: "#c28a20",
    incoming: "#356f9f",
    outgoing: "#2f7a4f",
    brightness: "1.02",
    saturation: "1.1",
  },
  dark: {
    schemeLabel: "深色",
    textBase: "#f1f5f4",
    textSecondaryBase: "#c4cdca",
    textMutedBase: "#929d99",
    surfaceBase: "#171d22",
    surfaceSubtleBase: "#1d252a",
    controlBase: "#252e33",
    borderBase: "#344047",
    dividerBase: "#2d373d",
    glassBase: "#11181d",
    overlay: "rgba(0, 0, 0, 0.48)",
    sidebarText: "#f3f7f5",
    sidebarMuted: "rgba(230, 239, 235, 0.74)",
    success: "#63c98c",
    warning: "#e2a950",
    danger: "#ef7d84",
    favorite: "#e8bd55",
    incoming: "#79b8ff",
    outgoing: "#63d08f",
    brightness: "0.94",
    saturation: "1.06",
  },
});

function clamp(value, minimum = 0, maximum = 1) {
  return Math.min(maximum, Math.max(minimum, value));
}

function hexToLinearRgb(hex) {
  const match = /^#([0-9a-f]{6})$/i.exec(String(hex));
  if (!match) throw new Error(`Unsupported palette color: ${hex}`);
  const channels = match[1].match(/.{2}/g).map((channel) =>
    Number.parseInt(channel, 16) / 255,
  );
  return channels.map((channel) =>
    channel <= 0.04045
      ? channel / 12.92
      : ((channel + 0.055) / 1.055) ** 2.4,
  );
}

function linearRgbToHex(rgb) {
  const channels = rgb.map((channel) => {
    const bounded = clamp(channel);
    const srgb =
      bounded <= 0.0031308
        ? bounded * 12.92
        : 1.055 * bounded ** (1 / 2.4) - 0.055;
    return Math.round(clamp(srgb) * 255)
      .toString(16)
      .padStart(2, "0");
  });
  return `#${channels.join("")}`;
}

function hexToOklch(hex) {
  const [red, green, blue] = hexToLinearRgb(hex);
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
  const chroma = Math.hypot(a, b);
  const hue = (Math.atan2(b, a) * 180) / Math.PI;
  return {
    lightness,
    chroma,
    hue: hue < 0 ? hue + 360 : hue,
  };
}

function linearRgbFromOklch(lightness, chroma, hue) {
  const radians = (hue * Math.PI) / 180;
  const a = chroma * Math.cos(radians);
  const b = chroma * Math.sin(radians);
  const l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (lightness - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}

function inSrgbGamut(rgb) {
  return rgb.every((channel) => channel >= 0 && channel <= 1);
}

function oklchToHex(lightness, chroma, hue) {
  const boundedLightness = clamp(lightness);
  let boundedChroma = Math.max(0, chroma);
  let rgb = linearRgbFromOklch(boundedLightness, boundedChroma, hue);
  if (!inSrgbGamut(rgb)) {
    let low = 0;
    let high = boundedChroma;
    for (let index = 0; index < 22; index += 1) {
      const candidate = (low + high) / 2;
      const candidateRgb = linearRgbFromOklch(
        boundedLightness,
        candidate,
        hue,
      );
      if (inSrgbGamut(candidateRgb)) {
        low = candidate;
        rgb = candidateRgb;
      } else {
        high = candidate;
      }
    }
    boundedChroma = low;
    rgb = linearRgbFromOklch(boundedLightness, boundedChroma, hue);
  }
  return linearRgbToHex(rgb);
}

function relativeLuminance(hex) {
  const [red, green, blue] = hexToLinearRgb(hex);
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}

function contrastRatio(first, second) {
  const firstLuminance = relativeLuminance(first);
  const secondLuminance = relativeLuminance(second);
  const lighter = Math.max(firstLuminance, secondLuminance);
  const darker = Math.min(firstLuminance, secondLuminance);
  return (lighter + 0.05) / (darker + 0.05);
}

function fitContrast(
  { lightness, chroma, hue },
  backgrounds,
  minimumRatio,
  direction,
) {
  let candidateLightness = lightness;
  for (let index = 0; index < 101; index += 1) {
    const candidate = oklchToHex(candidateLightness, chroma, hue);
    if (
      backgrounds.every(
        (background) => contrastRatio(candidate, background) >= minimumRatio,
      )
    ) {
      return candidate;
    }
    candidateLightness = clamp(
      candidateLightness + (direction === "lighter" ? 0.01 : -0.01),
    );
  }
  return direction === "lighter" ? "#ffffff" : "#000000";
}

function rgbaFromHex(hex, alpha) {
  const match = /^#([0-9a-f]{6})$/i.exec(hex);
  const channels = match[1]
    .match(/.{2}/g)
    .map((channel) => Number.parseInt(channel, 16));
  return `rgba(${channels.join(", ")}, ${alpha})`;
}

function accessibleOnAccent(accent) {
  const candidates = ["#050b10", "#ffffff"];
  return candidates.reduce((best, candidate) =>
    contrastRatio(candidate, accent) > contrastRatio(best, accent)
      ? candidate
      : best,
  );
}

function buildMinimalTokens(family, scheme, accent) {
  const dark = scheme === "dark";
  const surfaceSeed = hexToOklch(dark ? family.deep : family.soft);
  const warmSurface = surfaceSeed.hue >= 65 && surfaceSeed.hue <= 115;
  const surfaceColor = (lightness, chromaMultiplier, chromaLimit) =>
    oklchToHex(
      lightness,
      Math.min(
        surfaceSeed.chroma * chromaMultiplier,
        chromaLimit * (warmSurface ? 0.8 : 1),
      ),
      surfaceSeed.hue,
    );

  const canvas = dark
    ? surfaceColor(0.3, 0.5, 0.055)
    : surfaceColor(0.88, 0.42, 0.035);
  const sidebar = dark
    ? surfaceColor(0.34, 0.72, 0.08)
    : surfaceColor(0.84, 0.72, 0.06);
  const panel = dark
    ? surfaceColor(0.35, 0.36, 0.045)
    : surfaceColor(0.92, 0.26, 0.025);
  const panelSubtle = dark
    ? surfaceColor(0.325, 0.44, 0.05)
    : surfaceColor(0.9, 0.34, 0.03);
  const control = dark
    ? surfaceColor(0.38, 0.36, 0.045)
    : surfaceColor(0.87, 0.34, 0.035);
  const hover = dark
    ? surfaceColor(0.39, 0.44, 0.055)
    : surfaceColor(0.86, 0.44, 0.04);
  const selection = dark
    ? surfaceColor(0.42, 0.68, 0.075)
    : surfaceColor(0.82, 0.82, 0.075);
  const border = dark
    ? surfaceColor(0.48, 0.3, 0.035)
    : surfaceColor(0.7, 0.34, 0.04);
  const divider = dark
    ? surfaceColor(0.42, 0.26, 0.03)
    : surfaceColor(0.79, 0.26, 0.03);
  const edge = dark
    ? surfaceColor(0.5, 0.28, 0.035)
    : surfaceColor(0.74, 0.28, 0.035);
  const highlight = dark
    ? surfaceColor(0.58, 0.2, 0.025)
    : surfaceColor(0.97, 0.14, 0.018);
  const textBackgrounds = [
    canvas,
    sidebar,
    panel,
    panelSubtle,
    control,
    selection,
  ];
  const textDirection = dark ? "lighter" : "darker";
  const text = fitContrast(
    {
      lightness: dark ? 0.92 : 0.23,
      chroma: 0.012,
      hue: surfaceSeed.hue,
    },
    textBackgrounds,
    7,
    textDirection,
  );
  const textSecondary = fitContrast(
    {
      lightness: dark ? 0.78 : 0.39,
      chroma: 0.012,
      hue: surfaceSeed.hue,
    },
    textBackgrounds,
    4.5,
    textDirection,
  );
  const textMuted = fitContrast(
    {
      lightness: dark ? 0.68 : 0.46,
      chroma: 0.01,
      hue: surfaceSeed.hue,
    },
    textBackgrounds,
    4.5,
    textDirection,
  );
  const selectionBorder = fitContrast(
    {
      lightness: dark ? 0.58 : 0.6,
      chroma: Math.min(surfaceSeed.chroma * 0.72, warmSurface ? 0.08 : 0.1),
      hue: surfaceSeed.hue,
    },
    [selection],
    3,
    dark ? "lighter" : "darker",
  );
  const shadowSeed = dark ? family.deep : family.main;

  return Object.freeze({
    canvas,
    glass: panelSubtle,
    sidebar,
    panel,
    panelSubtle,
    control,
    text,
    textSecondary,
    textMuted,
    border,
    divider,
    selection,
    selectionBorder,
    hover,
    edge,
    highlight,
    sidebarText: text,
    sidebarMuted: textSecondary,
    onAccent: accessibleOnAccent(accent),
    panelShadow: dark
      ? `0 14px 34px ${rgbaFromHex(shadowSeed, 0.3)}, 0 2px 7px rgba(0, 0, 0, 0.2)`
      : `0 12px 30px ${rgbaFromHex(shadowSeed, 0.11)}, 0 2px 7px rgba(24, 35, 42, 0.08)`,
  });
}

function mix(color, amount, base) {
  return `color-mix(in srgb, ${color} ${amount}%, ${base})`;
}

function buildSemanticPalette(family, scheme) {
  const dark = scheme === "dark";
  const semantic = semanticByScheme[scheme];
  const accent = dark ? family.main : family.deep;
  const panel = mix(family.soft, dark ? 7 : 9, semantic.surfaceBase);
  const panelSubtle = mix(
    family.soft,
    dark ? 5 : 8,
    semantic.surfaceSubtleBase,
  );
  const canvas = mix(family.soft, dark ? 4 : 7, semantic.surfaceSubtleBase);
  const control = mix(family.soft, dark ? 7 : 11, semantic.controlBase);
  const glass = mix(
    dark ? family.deep : family.soft,
    dark ? 18 : 16,
    semantic.glassBase,
  );
  const text = mix(family.deep, dark ? 4 : 7, semantic.textBase);
  const textSecondary = mix(
    family.deep,
    dark ? 5 : 8,
    semantic.textSecondaryBase,
  );
  const textMuted = mix(family.deep, dark ? 5 : 7, semantic.textMutedBase);
  const border = mix(family.main, dark ? 12 : 14, semantic.borderBase);
  const divider = mix(family.main, dark ? 9 : 11, semantic.dividerBase);
  const selection = dark
    ? mix(family.deep, 38, "#1b2429")
    : mix(family.soft, 46, "#f6f9f8");
  const selectionBorder = dark
    ? mix(family.main, 44, "#35454d")
    : mix(family.main, 38, "#d4dfdc");
  const hover = dark
    ? mix(family.deep, 12, "#20282d")
    : mix(family.soft, 13, "#eff4f2");
  const edge = dark
    ? mix(family.soft, 18, "#dfe8e5")
    : mix(family.soft, 12, "#ffffff");
  const highlight = dark
    ? mix(family.soft, 10, "#eef5f2")
    : mix(family.soft, 8, "#ffffff");
  const accentHover = dark
    ? mix(family.soft, 66, family.main)
    : mix(family.deep, 86, "#000000");
  const accentPressed = dark
    ? mix(family.main, 82, "#ffffff")
    : mix(family.deep, 74, "#000000");
  const sidebarScrimTop = mix(family.deep, dark ? 24 : 16, "transparent");
  const sidebarScrimBottom = dark
    ? mix(family.deep, 68, "rgba(2, 8, 11, 0.72)")
    : mix(family.deep, 58, "rgba(7, 18, 22, 0.66)");
  const shadowColor = dark
    ? "rgba(0, 0, 0, 0.4)"
    : mix(family.deep, 20, "rgba(15, 30, 38, 0.18)");
  const semanticTint = dark ? 10 : 7;
  const success = mix(family.main, semanticTint, semantic.success);
  const warning = mix(family.main, semanticTint, semantic.warning);
  const danger = mix(family.main, semanticTint, semantic.danger);
  const favorite = mix(family.main, semanticTint, semantic.favorite);
  const incoming = mix(family.main, semanticTint, semantic.incoming);
  const outgoing = mix(family.main, semanticTint, semantic.outgoing);
  const chart = dark
    ? [family.main, "#56c3b8", "#a28ce6", warning, danger, "#98a3b1"]
    : [family.deep, "#2f9f98", "#806bc4", warning, danger, "#7f8a96"];

  const minimalTokens = buildMinimalTokens(family, scheme, accent);

  return Object.freeze({
    id: `${family.id}-${scheme}`,
    familyId: family.id,
    name: family.name,
    scheme,
    schemeLabel: semantic.schemeLabel,
    main: family.main,
    soft: family.soft,
    deep: family.deep,
    swatches: Object.freeze([glass, accent, selection, border]),
    minimalSwatches: Object.freeze([
      minimalTokens.sidebar,
      minimalTokens.canvas,
      minimalTokens.selection,
      accent,
    ]),
    minimalTokens,
    tokens: Object.freeze({
      scheme,
      canvas,
      glass,
      panel,
      panelSubtle,
      control,
      text,
      textSecondary,
      textMuted,
      border,
      divider,
      accent,
      accentSoft: family.soft,
      accentDeep: family.deep,
      accentHover,
      accentPressed,
      onAccent: dark ? "#10221c" : "#ffffff",
      selection,
      selectionBorder,
      hover,
      edge,
      highlight,
      overlay: semantic.overlay,
      sidebarText: semantic.sidebarText,
      sidebarMuted: semantic.sidebarMuted,
      sidebarScrimTop,
      sidebarScrimBottom,
      success,
      warning,
      danger,
      favorite,
      incoming,
      outgoing,
      chart,
      brightness: semantic.brightness,
      saturation: semantic.saturation,
      panelShadow: `0 16px 40px ${shadowColor}, 0 2px 8px ${mix(family.deep, dark ? 12 : 10, "transparent")}`,
    }),
  });
}

function buildThemePalette(spec) {
  const generated = buildSemanticPalette(spec, spec.scheme);
  const tokens = Object.freeze({ ...generated.tokens, ...spec.tokens });
  const minimalTokens = buildMinimalTokens(spec, spec.scheme, tokens.accent);
  return Object.freeze({
    ...generated,
    id: spec.id,
    familyId: `theme-${spec.id}`,
    name: spec.name,
    swatches: Object.freeze([
      tokens.glass,
      tokens.accent,
      tokens.selection,
      tokens.border,
    ]),
    minimalSwatches: Object.freeze([
      minimalTokens.sidebar,
      minimalTokens.canvas,
      minimalTokens.selection,
      tokens.accent,
    ]),
    minimalTokens,
    tokens,
  });
}

const builtinThemePalettes = Object.freeze([
  buildThemePalette({
    id: "daylight",
    name: "日间原色",
    scheme: "light",
    main: "#0878f9",
    soft: "#cfe4fb",
    deep: "#0062d5",
    tokens: {
      canvas: "#f6f8fa",
      glass: "#ecf4f8",
      panel: "#fcfcfd",
      panelSubtle: "#f6f8fa",
      control: "#f2f5f7",
      text: "#171a1f",
      textSecondary: "#414852",
      textMuted: "#727b86",
      border: "#d9dee5",
      divider: "#e1e5ea",
      accent: "#0878f9",
      accentSoft: "#cfe4fb",
      accentDeep: "#0062d5",
      accentHover: "#006dea",
      accentPressed: "#0062d5",
      onAccent: "#ffffff",
      selection: "#eaf4ff",
      selectionBorder: "#cfe4fb",
      hover: "#f2f5f8",
      success: "#31b56c",
      warning: "#aa6a13",
      danger: "#d34747",
      favorite: "#e4a82c",
      incoming: "#2563b8",
      outgoing: "#237a49",
      chart: ["#2f80dd", "#2f9f98", "#806bc4", "#d18a2f", "#ca626c", "#7f8a96"],
    },
  }),
  buildThemePalette({
    id: "night",
    name: "夜间原色",
    scheme: "dark",
    main: "#72aefc",
    soft: "#b8d7ff",
    deep: "#315278",
    tokens: {
      canvas: "#151a22",
      glass: "#111720",
      panel: "#171b23",
      panelSubtle: "#1d222c",
      control: "#232934",
      text: "#f2f4f7",
      textSecondary: "#c4cad3",
      textMuted: "#8f98a6",
      border: "#313945",
      divider: "#2a313c",
      accent: "#72aefc",
      accentSoft: "#b8d7ff",
      accentDeep: "#315278",
      accentHover: "#8abdff",
      accentPressed: "#5a9cf4",
      onAccent: "#10243c",
      selection: "#23344b",
      selectionBorder: "#315278",
      hover: "#202630",
      success: "#63c98c",
      warning: "#dda04d",
      danger: "#e9818a",
      favorite: "#e8bd55",
      incoming: "#79b8ff",
      outgoing: "#5fd08a",
      chart: ["#6ca7f4", "#56c3b8", "#a28ce6", "#e7b755", "#e9818a", "#98a3b1"],
    },
  }),
  buildThemePalette({
    id: "dusk",
    name: "黄昏原色",
    scheme: "light",
    main: "#c45f4f",
    soft: "#eccfc1",
    deep: "#913f34",
    tokens: {
      canvas: "#fbf2ea",
      glass: "#f7ede5",
      panel: "#fffaf5",
      panelSubtle: "#fbf2ea",
      control: "#f7ede5",
      text: "#2b2123",
      textSecondary: "#57484a",
      textMuted: "#837174",
      border: "#e6d5ce",
      divider: "#eaded8",
      accent: "#b95848",
      accentSoft: "#eccfc1",
      accentDeep: "#913f34",
      accentHover: "#a64b3d",
      accentPressed: "#913f34",
      onAccent: "#ffffff",
      selection: "#fae9df",
      selectionBorder: "#eccfc1",
      hover: "#fbf1eb",
      incoming: "#356f9f",
      outgoing: "#2f7a4f",
    },
  }),
  buildThemePalette({
    id: "forest",
    name: "森林原色",
    scheme: "light",
    main: "#347d58",
    soft: "#c6decf",
    deep: "#255d3f",
    tokens: {
      canvas: "#edf2eb",
      glass: "#e2eee3",
      panel: "#f5f8f3",
      panelSubtle: "#edf2eb",
      control: "#e8eee6",
      text: "#17231d",
      textSecondary: "#3e5046",
      textMuted: "#6e7c74",
      border: "#d1dbd2",
      divider: "#d9e1da",
      accent: "#357a55",
      accentSoft: "#c6decf",
      accentDeep: "#255d3f",
      accentHover: "#2d6c4a",
      accentPressed: "#255d3f",
      onAccent: "#ffffff",
      selection: "#e1eee5",
      selectionBorder: "#c6decf",
      hover: "#eaf0e9",
      incoming: "#306ba6",
      outgoing: "#2b744b",
    },
  }),
]);

export const appearancePalettes = Object.freeze([
  ...builtinThemePalettes,
  ...["light", "dark"].flatMap((scheme) =>
    paletteFamilies.map((family) => buildSemanticPalette(family, scheme)),
  ),
]);

const paletteIds = new Set(appearancePalettes.map((palette) => palette.id));
const familyIds = new Set(paletteFamilies.map((family) => family.id));

export function normalizeAppearancePaletteId(value, legacyMode = "light") {
  const id = String(value || "");
  if (paletteIds.has(id)) return id;
  if (familyIds.has(id)) {
    const scheme = legacyMode === "dark" ? "dark" : "light";
    return `${id}-${scheme}`;
  }
  return "daylight";
}

export const builtinAppearanceThemes = Object.freeze([
  {
    id: "daylight",
    name: "日间",
    englishName: "Daylight",
    paletteId: "daylight",
  },
  {
    id: "night",
    name: "夜间",
    englishName: "Night",
    paletteId: "night",
  },
  {
    id: "dusk",
    name: "黄昏",
    englishName: "Dusk",
    paletteId: "dusk",
  },
  {
    id: "forest",
    name: "森林",
    englishName: "Forest",
    paletteId: "forest",
  },
]);

export const defaultAppearance = Object.freeze({
  selectionInitialized: false,
  paletteId: "daylight",
  minimalModeEnabled: true,
  activeTheme: { kind: "builtin", id: "daylight" },
  customPresets: [],
  activeBackgroundDataUrl: null,
});

export function appearanceFromLegacyTheme(theme) {
  const builtin = builtinAppearanceThemes.find((item) => item.id === theme);
  return {
    ...defaultAppearance,
    paletteId: builtin?.paletteId || "daylight",
    minimalModeEnabled: theme == null,
    activeTheme: { kind: "builtin", id: builtin?.id || "daylight" },
  };
}

export function appearanceFromSavedAppearance(storage = window.localStorage) {
  const legacyTheme = storage.getItem("mine-mail-theme");
  const appearance = appearanceFromLegacyTheme(legacyTheme);
  const explicitPalette = storage.getItem("mine-mail-appearance-palette");
  const legacyCustomPalette = storage.getItem("mine-mail-custom-palette");
  const explicitMinimal = storage.getItem("mine-mail-minimal-mode");
  return {
    ...appearance,
    paletteId: normalizeAppearancePaletteId(
      explicitPalette || legacyCustomPalette || appearance.paletteId,
    ),
    minimalModeEnabled:
      explicitMinimal == null
        ? appearance.minimalModeEnabled
        : explicitMinimal === "true",
  };
}

export function activeCustomPreset(appearance) {
  if (appearance?.activeTheme?.kind !== "custom") return null;
  return (
    appearance.customPresets?.find(
      (preset) => preset.id === appearance.activeTheme.id,
    ) || null
  );
}

const paletteVariables = Object.freeze({
  scheme: "--palette-color-scheme",
  canvas: "--palette-canvas",
  glass: "--palette-glass",
  panel: "--palette-panel",
  panelSubtle: "--palette-panel-subtle",
  control: "--palette-control",
  text: "--palette-text",
  textSecondary: "--palette-text-secondary",
  textMuted: "--palette-text-muted",
  border: "--palette-border",
  divider: "--palette-divider",
  accent: "--palette-accent",
  accentSoft: "--palette-accent-soft",
  accentDeep: "--palette-accent-deep",
  accentHover: "--palette-accent-hover",
  accentPressed: "--palette-accent-pressed",
  onAccent: "--palette-on-accent",
  selection: "--palette-selection",
  selectionBorder: "--palette-selection-border",
  hover: "--palette-hover",
  edge: "--palette-edge",
  highlight: "--palette-highlight",
  overlay: "--palette-overlay",
  sidebarText: "--palette-sidebar-text",
  sidebarMuted: "--palette-sidebar-muted",
  sidebarScrimTop: "--palette-sidebar-scrim-top",
  sidebarScrimBottom: "--palette-sidebar-scrim-bottom",
  success: "--palette-success",
  warning: "--palette-warning",
  danger: "--palette-danger",
  favorite: "--palette-favorite",
  incoming: "--palette-incoming",
  outgoing: "--palette-outgoing",
  brightness: "--palette-brightness",
  saturation: "--palette-saturation",
  panelShadow: "--palette-panel-shadow",
});

const minimalPaletteVariables = Object.freeze({
  canvas: "--palette-minimal-canvas",
  glass: "--palette-minimal-glass",
  sidebar: "--palette-minimal-sidebar",
  panel: "--palette-minimal-panel",
  panelSubtle: "--palette-minimal-panel-subtle",
  control: "--palette-minimal-control",
  text: "--palette-minimal-text",
  textSecondary: "--palette-minimal-text-secondary",
  textMuted: "--palette-minimal-text-muted",
  border: "--palette-minimal-border",
  divider: "--palette-minimal-divider",
  onAccent: "--palette-minimal-on-accent",
  selection: "--palette-minimal-selection",
  selectionBorder: "--palette-minimal-selection-border",
  hover: "--palette-minimal-hover",
  edge: "--palette-minimal-edge",
  highlight: "--palette-minimal-highlight",
  sidebarText: "--palette-minimal-sidebar-text",
  sidebarMuted: "--palette-minimal-sidebar-muted",
  panelShadow: "--palette-minimal-panel-shadow",
});

export function applyAppearancePaletteToRoot(root, paletteId) {
  const normalizedId = normalizeAppearancePaletteId(paletteId);
  const palette =
    appearancePalettes.find((item) => item.id === normalizedId) ||
    appearancePalettes.find((item) => item.id === "daylight");
  Object.entries(paletteVariables).forEach(([token, property]) => {
    root.style.setProperty(property, palette.tokens[token]);
  });
  Object.entries(minimalPaletteVariables).forEach(([token, property]) => {
    root.style.setProperty(property, palette.minimalTokens[token]);
  });
  palette.tokens.chart.forEach((color, index) => {
    root.style.setProperty(`--palette-chart-${index + 1}`, color);
  });
  return palette;
}

function clearCustomBackground(root) {
  root.style.removeProperty("--custom-wallpaper");
  root.style.removeProperty("--custom-focal-x");
  root.style.removeProperty("--custom-focal-y");
}

export function applyAppearanceToDocument(appearance) {
  const root = document.documentElement;
  const active = appearance?.activeTheme || defaultAppearance.activeTheme;
  const preset = activeCustomPreset(appearance);
  const palette = applyAppearancePaletteToRoot(
    root,
    appearance?.paletteId || defaultAppearance.paletteId,
  );
  const minimalModeEnabled = appearance?.minimalModeEnabled !== false;
  root.dataset.appearanceMode = minimalModeEnabled ? "minimal" : "image";
  if (active.kind !== "custom" || !preset) {
    const theme = builtinAppearanceThemes.some((item) => item.id === active.id)
      ? active.id
      : "daylight";
    root.dataset.theme = theme;
    delete root.dataset.colorMode;
    clearCustomBackground(root);
    window.localStorage.setItem("mine-mail-theme", theme);
    if (!minimalModeEnabled && palette.id === theme) {
      root.dataset.paletteSource = "theme-original";
    } else {
      root.dataset.paletteSource = "selected";
    }
  } else {
    root.dataset.theme = "custom";
    root.dataset.paletteSource = "selected";
    delete root.dataset.colorMode;
    root.style.setProperty(
      "--custom-wallpaper",
      appearance.activeBackgroundDataUrl
        ? `url("${appearance.activeBackgroundDataUrl}")`
        : "none",
    );
    root.style.setProperty("--custom-focal-x", `${preset.focalX * 100}%`);
    root.style.setProperty("--custom-focal-y", `${preset.focalY * 100}%`);
    window.localStorage.setItem("mine-mail-theme", "custom");
  }

  window.localStorage.setItem("mine-mail-appearance-palette", palette.id);
  window.localStorage.setItem(
    "mine-mail-minimal-mode",
    String(minimalModeEnabled),
  );
  window.localStorage.removeItem("mine-mail-custom-mode");
  window.localStorage.removeItem("mine-mail-custom-palette");
}
