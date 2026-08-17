const paletteFamilies = Object.freeze([
  { id: "green", name: "青绿", main: "#79b96a", soft: "#b8e89d", deep: "#4e7f43" },
  { id: "teal", name: "薄荷", main: "#66d3b6", soft: "#9cebd6", deep: "#198f76" },
  { id: "cyan", name: "湖蓝", main: "#67c7d8", soft: "#9ae2eb", deep: "#338b9d" },
  { id: "sky", name: "晴蓝", main: "#78b9ee", soft: "#b5dcfa", deep: "#3e7fb4" },
  { id: "blue", name: "群青", main: "#858fe7", soft: "#c1c8fa", deep: "#545eae" },
  { id: "indigo", name: "靛青", main: "#9c7fdf", soft: "#d1bdf6", deep: "#6b4cac" },
  { id: "violet", name: "紫罗兰", main: "#b37bd8", soft: "#dfb9f0", deep: "#7b49a0" },
  { id: "purple", name: "木槿", main: "#cc7ed5", soft: "#edb9ee", deep: "#934a9b" },
  { id: "magenta", name: "莓粉", main: "#df82b0", soft: "#f5bdd7", deep: "#a64d78" },
  { id: "rose", name: "珊瑚", main: "#e98c7a", soft: "#ffc3b5", deep: "#b15549" },
  { id: "orange", name: "暖橙", main: "#e9a24f", soft: "#ffd19a", deep: "#ad6c25" },
  { id: "yellow", name: "麦黄", main: "#d4c34f", soft: "#f3e781", deep: "#948529" },
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

function mix(color, amount, base) {
  return `color-mix(in srgb, ${color} ${amount}%, ${base})`;
}

function buildSemanticPalette(family, scheme) {
  const dark = scheme === "dark";
  const semantic = semanticByScheme[scheme];
  const accent = dark ? family.main : family.deep;
  const panel = mix(family.soft, dark ? 7 : 9, semantic.surfaceBase);
  const panelSubtle = mix(family.soft, dark ? 5 : 8, semantic.surfaceSubtleBase);
  const control = mix(family.soft, dark ? 7 : 11, semantic.controlBase);
  const glass = mix(dark ? family.deep : family.soft, dark ? 18 : 16, semantic.glassBase);
  const text = mix(family.deep, dark ? 4 : 7, semantic.textBase);
  const textSecondary = mix(family.deep, dark ? 5 : 8, semantic.textSecondaryBase);
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
    tokens: Object.freeze({
      scheme,
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

export const appearancePalettes = Object.freeze(
  ["light", "dark"].flatMap((scheme) =>
    paletteFamilies.map((family) => buildSemanticPalette(family, scheme)),
  ),
);

const paletteIds = new Set(appearancePalettes.map((palette) => palette.id));
const familyIds = new Set(paletteFamilies.map((family) => family.id));

export function normalizeAppearancePaletteId(value, legacyMode = "light") {
  const id = String(value || "");
  if (paletteIds.has(id)) return id;
  if (familyIds.has(id)) {
    const scheme = legacyMode === "dark" ? "dark" : "light";
    return `${id}-${scheme}`;
  }
  return "sky-light";
}

export const builtinAppearanceThemes = Object.freeze([
  {
    id: "daylight",
    name: "日间",
    englishName: "Daylight",
    paletteId: "sky-light",
  },
  {
    id: "night",
    name: "夜间",
    englishName: "Night",
    paletteId: "sky-dark",
  },
  {
    id: "dusk",
    name: "黄昏",
    englishName: "Dusk",
    paletteId: "rose-light",
  },
  {
    id: "forest",
    name: "森林",
    englishName: "Forest",
    paletteId: "green-light",
  },
]);

export const defaultAppearance = Object.freeze({
  selectionInitialized: false,
  activeTheme: { kind: "builtin", id: "daylight" },
  customPresets: [],
  activeBackgroundDataUrl: null,
});

export function appearanceFromLegacyTheme(theme) {
  const valid = builtinAppearanceThemes.some((item) => item.id === theme);
  return {
    ...defaultAppearance,
    activeTheme: { kind: "builtin", id: valid ? theme : "daylight" },
  };
}

export function activeCustomPreset(appearance) {
  if (appearance?.activeTheme?.kind !== "custom") return null;
  return appearance.customPresets?.find(
    (preset) => preset.id === appearance.activeTheme.id,
  ) || null;
}

const customPaletteVariables = Object.freeze({
  scheme: "--custom-color-scheme",
  glass: "--custom-glass",
  panel: "--custom-panel",
  panelSubtle: "--custom-panel-subtle",
  control: "--custom-control",
  text: "--custom-text",
  textSecondary: "--custom-text-secondary",
  textMuted: "--custom-text-muted",
  border: "--custom-border",
  divider: "--custom-divider",
  accent: "--custom-accent",
  accentSoft: "--custom-accent-soft",
  accentDeep: "--custom-accent-deep",
  accentHover: "--custom-accent-hover",
  accentPressed: "--custom-accent-pressed",
  onAccent: "--custom-on-accent",
  selection: "--custom-selection",
  selectionBorder: "--custom-selection-border",
  hover: "--custom-hover",
  edge: "--custom-edge",
  highlight: "--custom-highlight",
  overlay: "--custom-overlay",
  sidebarText: "--custom-sidebar-text",
  sidebarMuted: "--custom-sidebar-muted",
  sidebarScrimTop: "--custom-sidebar-scrim-top",
  sidebarScrimBottom: "--custom-sidebar-scrim-bottom",
  success: "--custom-success",
  warning: "--custom-warning",
  danger: "--custom-danger",
  favorite: "--custom-favorite",
  incoming: "--custom-incoming",
  outgoing: "--custom-outgoing",
  brightness: "--custom-brightness",
  saturation: "--custom-saturation",
  panelShadow: "--custom-panel-shadow",
});

function clearCustomPalette(root) {
  Object.values(customPaletteVariables).forEach((property) => {
    root.style.removeProperty(property);
  });
  for (let index = 0; index < 6; index += 1) {
    root.style.removeProperty(`--custom-chart-${index + 1}`);
  }
  root.style.removeProperty("--custom-wallpaper");
  root.style.removeProperty("--custom-focal-x");
  root.style.removeProperty("--custom-focal-y");
}

export function applyAppearanceToDocument(appearance) {
  const root = document.documentElement;
  const active = appearance?.activeTheme || defaultAppearance.activeTheme;
  const preset = activeCustomPreset(appearance);
  if (active.kind !== "custom" || !preset) {
    const theme = builtinAppearanceThemes.some((item) => item.id === active.id)
      ? active.id
      : "daylight";
    root.dataset.theme = theme;
    delete root.dataset.colorMode;
    clearCustomPalette(root);
    window.localStorage.setItem("mine-mail-theme", theme);
    window.localStorage.removeItem("mine-mail-custom-mode");
    window.localStorage.removeItem("mine-mail-custom-palette");
    return;
  }

  const legacyMode =
    preset.effectiveMode || (preset.mode === "dark" ? "dark" : "light");
  const paletteId = normalizeAppearancePaletteId(preset.paletteId, legacyMode);
  const palette =
    appearancePalettes.find((item) => item.id === paletteId) ||
    appearancePalettes.find((item) => item.id === "sky-light");
  root.dataset.theme = "custom";
  delete root.dataset.colorMode;
  Object.entries(customPaletteVariables).forEach(([token, property]) => {
    root.style.setProperty(property, palette.tokens[token]);
  });
  palette.tokens.chart.forEach((color, index) => {
    root.style.setProperty(`--custom-chart-${index + 1}`, color);
  });
  root.style.setProperty(
    "--custom-wallpaper",
    appearance.activeBackgroundDataUrl
      ? `url("${appearance.activeBackgroundDataUrl}")`
      : "none",
  );
  root.style.setProperty("--custom-focal-x", `${preset.focalX * 100}%`);
  root.style.setProperty("--custom-focal-y", `${preset.focalY * 100}%`);
  window.localStorage.setItem("mine-mail-theme", "custom");
  window.localStorage.removeItem("mine-mail-custom-mode");
  window.localStorage.setItem("mine-mail-custom-palette", palette.id);
}
