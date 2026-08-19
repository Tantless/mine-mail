import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { appearancePalettes } from "../appearanceThemes.js";
import { AppearanceSettings } from "./AppearanceSettings.jsx";

const appearance = {
  selectionInitialized: true,
  paletteId: "teal-light",
  minimalModeEnabled: true,
  activeTheme: { kind: "custom", id: "preset-1" },
  customPresets: [
    {
      id: "preset-1",
      name: "海岸",
      focalX: 0.5,
      focalY: 0.5,
      thumbnailDataUrl: "data:image/jpeg;base64,AQID",
    },
  ],
  activeBackgroundDataUrl: "data:image/jpeg;base64,BAUG",
};

const builtinAppearance = {
  selectionInitialized: true,
  paletteId: "night",
  minimalModeEnabled: false,
  activeTheme: { kind: "builtin", id: "night" },
  customPresets: appearance.customPresets,
  activeBackgroundDataUrl: null,
};

function props(overrides = {}) {
  return {
    appearance,
    idlePoetryEnabled: true,
    onIdlePoetryEnabledChange: vi.fn(),
    onSelect: vi.fn().mockResolvedValue(appearance),
    onUpdatePreferences: vi.fn().mockResolvedValue(appearance),
    onImport: vi.fn().mockResolvedValue(appearance),
    onUpdate: vi.fn().mockResolvedValue(appearance),
    onDelete: vi.fn().mockResolvedValue(appearance),
    ...overrides,
  };
}

function normalizedBackground(color) {
  const sample = document.createElement("span");
  sample.style.background = color;
  return sample.style.background;
}

function palettePreview(button) {
  return Array.from(
    button.querySelectorAll(".appearance-palette__segment"),
    (segment) => segment.style.background,
  );
}

describe("AppearanceSettings", () => {
  afterEach(cleanup);

  it("keeps four fixed themes before custom presets and the add card", () => {
    render(<AppearanceSettings {...props()} />);

    const themeButtons = [
      screen.getByRole("button", { name: "使用日间主题" }),
      screen.getByRole("button", { name: "使用夜间主题" }),
      screen.getByRole("button", { name: "使用黄昏主题" }),
      screen.getByRole("button", { name: "使用森林主题" }),
      screen.getByRole("button", { name: "使用海岸主题" }),
      screen.getByRole("button", { name: "添加自定义主题" }),
    ];
    const positions = themeButtons.map((button) =>
      Array.from(button.closest(".appearance-theme-grid").children).indexOf(
        button.closest(".appearance-theme-card-wrap") ||
          button.closest(".appearance-theme-card") ||
          button,
      ),
    );
    expect(positions).toEqual([0, 1, 2, 3, 4, 5]);
    expect(screen.queryByText(/设置只作用于这一个预设/)).toBeNull();
    expect(screen.queryByText(/图片会保存在 Mine Mail/)).toBeNull();
  });

  it("selects curated palettes and persists a normalized focal point", async () => {
    const user = userEvent.setup();
    const onUpdate = vi.fn().mockResolvedValue(appearance);
    const onUpdatePreferences = vi.fn().mockResolvedValue(appearance);
    render(<AppearanceSettings {...props({ onUpdate, onUpdatePreferences })} />);

    await user.click(screen.getByRole("button", { name: /^界面配色/ }));
    await user.click(screen.getByRole("button", { name: "珊瑚明亮调色板" }));
    expect(onUpdatePreferences).toHaveBeenCalledWith({
      paletteId: "rose-light",
    });

    await user.click(screen.getByRole("button", { name: /^设置焦点/ }));
    const preview = screen.getByRole("button", {
      name: "背景焦点。点击图片设置希望保持可见的位置",
    });
    vi.spyOn(preview, "getBoundingClientRect").mockReturnValue({
      left: 10,
      top: 20,
      width: 200,
      height: 100,
      right: 210,
      bottom: 120,
      x: 10,
      y: 20,
      toJSON: () => ({}),
    });
    fireEvent.pointerDown(preview, { clientX: 60, clientY: 95 });

    await waitFor(() =>
      expect(onUpdate).toHaveBeenLastCalledWith({
        id: "preset-1",
        focalX: 0.25,
        focalY: 0.75,
      }),
    );
  });

  it("offers every built-in theme's original palette in minimal mode", async () => {
    const user = userEvent.setup();
    const onUpdatePreferences = vi.fn().mockResolvedValue(appearance);
    render(<AppearanceSettings {...props({ onUpdatePreferences })} />);

    await user.click(screen.getByRole("button", { name: /^界面配色/ }));
    for (const name of ["日间原色", "夜间原色", "黄昏原色", "森林原色"]) {
      expect(screen.getByRole("button", { name: new RegExp(`^${name}`) })).toBeTruthy();
    }
    await user.click(screen.getByRole("button", { name: /^森林原色/ }));
    expect(onUpdatePreferences).toHaveBeenCalledWith({ paletteId: "forest" });
  });

  it("previews the active material mode with the same palette identity", async () => {
    const user = userEvent.setup();
    const teal = appearancePalettes.find((palette) => palette.id === "teal-light");
    render(<AppearanceSettings {...props()} />);

    await user.click(screen.getByRole("button", { name: /^界面配色/ }));
    expect(
      palettePreview(
        screen.getByRole("button", { name: "薄荷明亮调色板" }),
      ),
    ).toEqual(teal.minimalSwatches.map(normalizedBackground));

    cleanup();
    render(
      <AppearanceSettings
        {...props({
          appearance: { ...appearance, minimalModeEnabled: false },
        })}
      />,
    );
    await user.click(screen.getByRole("button", { name: /^界面配色/ }));
    expect(
      palettePreview(
        screen.getByRole("button", { name: "薄荷明亮调色板" }),
      ),
    ).toEqual(teal.swatches.map(normalizedBackground));
  });

  it("keeps the global palette editable and hides focal controls for built-in backgrounds", async () => {
    const user = userEvent.setup();
    const onUpdatePreferences = vi.fn().mockResolvedValue(builtinAppearance);
    render(
      <AppearanceSettings
        {...props({ appearance: builtinAppearance, onUpdatePreferences })}
      />,
    );

    expect(screen.queryByRole("button", { name: /^设置焦点/ })).toBeNull();
    const paletteControl = screen.getByRole("button", { name: /^界面配色/ });
    expect(paletteControl.disabled).toBe(false);
    await user.click(paletteControl);
    await user.click(screen.getByRole("button", { name: "薄荷明亮调色板" }));
    expect(onUpdatePreferences).toHaveBeenCalledWith({ paletteId: "teal-light" });
    expect(screen.queryByRole("checkbox", { name: /夜间模式/ })).toBeNull();
  });

  it("selects a complete dark palette without a separate night-mode switch", async () => {
    const user = userEvent.setup();
    const onUpdatePreferences = vi.fn().mockResolvedValue(appearance);
    render(<AppearanceSettings {...props({ onUpdatePreferences })} />);

    await user.click(screen.getByRole("button", { name: /^界面配色/ }));
    await user.click(screen.getByRole("button", { name: "晴蓝深色调色板" }));

    expect(onUpdatePreferences).toHaveBeenCalledWith({
      paletteId: "sky-dark",
    });
    expect(screen.queryByRole("checkbox", { name: /夜间模式/ })).toBeNull();
  });

  it("toggles minimal mode without changing the selected background", async () => {
    const user = userEvent.setup();
    const onUpdatePreferences = vi.fn().mockResolvedValue(appearance);
    const onSelect = vi.fn().mockResolvedValue(appearance);
    render(<AppearanceSettings {...props({ onUpdatePreferences, onSelect })} />);

    const toggle = screen.getByRole("checkbox", { name: "启用极简模式" });
    expect(toggle.checked).toBe(true);
    await user.click(toggle);
    expect(onUpdatePreferences).toHaveBeenCalledWith({
      minimalModeEnabled: false,
    });
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("exposes the homepage poetry preference as an independent appearance bar", async () => {
    const user = userEvent.setup();
    const onIdlePoetryEnabledChange = vi.fn();
    render(
      <AppearanceSettings
        {...props({ onIdlePoetryEnabledChange })}
      />,
    );

    const toggle = screen.getByRole("checkbox", { name: "展示主页诗歌" });
    expect(toggle.checked).toBe(true);
    await user.click(toggle);
    expect(onIdlePoetryEnabledChange).toHaveBeenCalledWith(false);
  });

  it("imports a supported image through a bounded encoded payload", async () => {
    const user = userEvent.setup();
    const onImport = vi.fn().mockResolvedValue(appearance);
    render(<AppearanceSettings {...props({ onImport })} />);
    const file = new File([new Uint8Array([137, 80, 78, 71])], "scene.png", {
      type: "image/png",
    });

    const input = screen.getByLabelText("选择自定义背景图片");
    expect(input.className).toBe("sr-only");
    expect(input.tabIndex).toBe(-1);
    await user.upload(input, file);

    await waitFor(() => expect(onImport).toHaveBeenCalledTimes(1));
    const request = onImport.mock.calls[0][0];
    expect(request.imageDataUrl).toMatch(/^data:image\/png;base64,/);
    expect(request).not.toHaveProperty("imageBytes");
  });

  it("confirms custom preset deletion without browser-native dialogs", async () => {
    const user = userEvent.setup();
    const onDelete = vi.fn().mockResolvedValue(appearance);
    render(<AppearanceSettings {...props({ onDelete })} />);

    await user.click(screen.getByRole("button", { name: "管理海岸" }));
    await user.click(screen.getByRole("menuitem", { name: "删除" }));
    expect(screen.getByRole("dialog", { name: "删除“海岸”？" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "删除主题" }));

    expect(onDelete).toHaveBeenCalledWith("preset-1");
  });

  it("toggles the theme schedule and reveals its three time rows", async () => {
    const user = userEvent.setup();
    const onThemeScheduleChange = vi.fn();
    render(
      <AppearanceSettings
        {...props({
          themeScheduleEnabled: false,
          themeScheduleDayStart: "06:00",
          themeScheduleDuskStart: "18:00",
          themeScheduleNightStart: "21:00",
          onThemeScheduleChange,
        })}
      />,
    );

    const toggle = screen.getByRole("checkbox", { name: "定时切换主题背景" });
    expect(toggle.checked).toBe(false);
    expect(screen.queryByRole("combobox", { name: /日间开始小时/ })).toBeNull();

    await user.click(toggle);
    expect(onThemeScheduleChange).toHaveBeenCalledWith({
      themeScheduleEnabled: true,
    });
  });

  it("shows the configured time rows while enabled and emits partial updates", async () => {
    const user = userEvent.setup();
    const onThemeScheduleChange = vi.fn();
    render(
      <AppearanceSettings
        {...props({
          themeScheduleEnabled: true,
          themeScheduleDayStart: "07:00",
          themeScheduleDuskStart: "18:30",
          themeScheduleNightStart: "21:00",
          onThemeScheduleChange,
        })}
      />,
    );

    expect(
      screen.getByRole("combobox", { name: "日间开始小时" }).textContent,
    ).toContain("07");
    expect(
      screen.getByRole("combobox", { name: "黄昏开始分钟" }).textContent,
    ).toContain("30");

    await user.click(screen.getByRole("combobox", { name: "夜间开始小时" }));
    await user.click(await screen.findByRole("option", { name: "22" }));
    expect(onThemeScheduleChange).toHaveBeenLastCalledWith({
      themeScheduleNightStart: "22:00",
    });
  });

  it("keeps the time rows hidden while the schedule is disabled", () => {
    render(
      <AppearanceSettings
        {...props({
          themeScheduleEnabled: false,
          themeScheduleDayStart: "06:00",
          themeScheduleDuskStart: "18:00",
          themeScheduleNightStart: "21:00",
          onThemeScheduleChange: vi.fn(),
        })}
      />,
    );

    expect(screen.queryByRole("combobox", { name: /开始小时/ })).toBeNull();
    expect(screen.queryByRole("combobox", { name: /开始分钟/ })).toBeNull();
  });

  it("rejects an unordered schedule with an inline hint and no save", async () => {
    const user = userEvent.setup();
    const onThemeScheduleChange = vi.fn();
    render(
      <AppearanceSettings
        {...props({
          themeScheduleEnabled: true,
          themeScheduleDayStart: "06:00",
          themeScheduleDuskStart: "18:00",
          themeScheduleNightStart: "21:00",
          onThemeScheduleChange,
        })}
      />,
    );

    await user.click(screen.getByRole("combobox", { name: "黄昏开始小时" }));
    await user.click(await screen.findByRole("option", { name: "05" }));

    expect(onThemeScheduleChange).not.toHaveBeenCalled();
    expect(
      screen.getByRole("alert").textContent,
    ).toContain("日间开始时间需要早于黄昏开始时间");
  });

  it("clears the schedule issue once the times are ordered again", async () => {
    const user = userEvent.setup();
    const onThemeScheduleChange = vi.fn();
    render(
      <AppearanceSettings
        {...props({
          themeScheduleEnabled: true,
          themeScheduleDayStart: "06:00",
          themeScheduleDuskStart: "18:00",
          themeScheduleNightStart: "21:00",
          onThemeScheduleChange,
        })}
      />,
    );

    await user.click(screen.getByRole("combobox", { name: "黄昏开始小时" }));
    await user.click(await screen.findByRole("option", { name: "05" }));
    expect(
      screen.getByRole("alert").textContent,
    ).toContain("日间开始时间需要早于黄昏开始时间");

    await user.click(screen.getByRole("combobox", { name: "日间开始小时" }));
    await user.click(await screen.findByRole("option", { name: "04" }));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(onThemeScheduleChange).toHaveBeenLastCalledWith({
      themeScheduleDayStart: "04:00",
    });
  });
});
