import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppearanceSettings } from "./AppearanceSettings.jsx";

const appearance = {
  selectionInitialized: true,
  activeTheme: { kind: "custom", id: "preset-1" },
  customPresets: [
    {
      id: "preset-1",
      name: "海岸",
      paletteId: "teal-light",
      focalX: 0.5,
      focalY: 0.5,
      thumbnailDataUrl: "data:image/jpeg;base64,AQID",
    },
  ],
  activeBackgroundDataUrl: "data:image/jpeg;base64,BAUG",
};

const builtinAppearance = {
  selectionInitialized: true,
  activeTheme: { kind: "builtin", id: "night" },
  customPresets: appearance.customPresets,
  activeBackgroundDataUrl: null,
};

function props(overrides = {}) {
  return {
    appearance,
    onSelect: vi.fn().mockResolvedValue(appearance),
    onImport: vi.fn().mockResolvedValue(appearance),
    onUpdate: vi.fn().mockResolvedValue(appearance),
    onDelete: vi.fn().mockResolvedValue(appearance),
    ...overrides,
  };
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
    render(<AppearanceSettings {...props({ onUpdate })} />);

    await user.click(screen.getByRole("button", { name: /^调色盘/ }));
    await user.click(screen.getByRole("button", { name: "珊瑚明亮调色板" }));
    expect(onUpdate).toHaveBeenCalledWith({
      id: "preset-1",
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

  it("keeps both theme controls visible and disabled for built-in themes", () => {
    render(
      <AppearanceSettings
        {...props({ appearance: builtinAppearance })}
      />,
    );

    expect(screen.getByRole("button", { name: /^设置焦点/ }).disabled).toBe(true);
    expect(screen.getByRole("button", { name: /^调色盘/ }).disabled).toBe(true);
    expect(screen.queryByRole("checkbox", { name: /夜间模式/ })).toBeNull();
    expect(screen.queryByText(/内置主题，配置仅供查看/)).toBeNull();
  });

  it("selects a complete dark palette without a separate night-mode switch", async () => {
    const user = userEvent.setup();
    const onUpdate = vi.fn().mockResolvedValue(appearance);
    render(<AppearanceSettings {...props({ onUpdate })} />);

    await user.click(screen.getByRole("button", { name: /^调色盘/ }));
    await user.click(screen.getByRole("button", { name: "晴蓝深色调色板" }));

    expect(onUpdate).toHaveBeenCalledWith({
      id: "preset-1",
      paletteId: "sky-dark",
    });
    expect(screen.queryByRole("checkbox", { name: /夜间模式/ })).toBeNull();
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
});
