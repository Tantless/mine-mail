import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AgentSettings } from "./AgentSettings.jsx";

const presets = [
  {
    id: "custom",
    label: "自定义",
    baseUrl: "",
    environmentVariable: "AI_API_KEY",
    models: [],
  },
  {
    id: "deepseek",
    label: "DeepSeek",
    baseUrl: "https://api.deepseek.com",
    environmentVariable: "DEEPSEEK_API_KEY",
    models: ["deepseek-v4-flash", "deepseek-v4-pro"],
  },
  {
    id: "kimi",
    label: "Kimi",
    baseUrl: "https://api.moonshot.cn/v1",
    environmentVariable: "MOONSHOT_API_KEY",
    models: ["kimi-k2.6", "kimi-k3"],
  },
];

function configuration(overrides = {}) {
  return {
    providerId: "deepseek",
    baseUrl: "https://api.deepseek.com",
    modelName: "deepseek-v4-pro",
    useEnvironmentKey: false,
    hasStoredApiKey: true,
    hasEnvironmentApiKey: false,
    environmentVariable: "DEEPSEEK_API_KEY",
    presets,
    translationLanguage: "zh-Hans",
    translationLanguages: [
      { value: "zh-Hans", label: "中文（简体）" },
      { value: "zh-Hant", label: "中文（繁體）" },
      { value: "en", label: "English" },
      { value: "ja", label: "日本語" },
      { value: "ru", label: "Русский" },
      { value: "es", label: "Español" },
      { value: "fr", label: "Français" },
    ],
    ...overrides,
  };
}

function client(overrides = {}) {
  return {
    getAiConfig: vi.fn().mockResolvedValue(configuration()),
    saveAiConfig: vi.fn().mockImplementation(async (request) =>
      configuration({
        ...request,
        hasStoredApiKey: true,
        environmentVariable:
          presets.find((preset) => preset.id === request.providerId)
            ?.environmentVariable || "AI_API_KEY",
      }),
    ),
    listAiModels: vi
      .fn()
      .mockResolvedValue(["deepseek-v4-flash", "deepseek-v4-pro"]),
    testAiConnection: vi.fn().mockResolvedValue({ latencyMs: 86 }),
    ...overrides,
  };
}

describe("AgentSettings", () => {
  afterEach(() => cleanup());

  it("offers preset models immediately and replaces them after retrieval", async () => {
    const user = userEvent.setup();
    const api = client({
      listAiModels: vi.fn().mockResolvedValue(["kimi-k2.7", "kimi-k3"]),
    });
    render(<AgentSettings client={api} />);

    await screen.findByRole("button", { name: "Kimi" });
    await user.click(screen.getByRole("button", { name: "Kimi" }));
    expect(screen.getByLabelText("BASE_URL").value).toBe(
      "https://api.moonshot.cn/v1",
    );
    expect(screen.getByLabelText("MODEL_NAME").value).toBe("kimi-k2.6");
    const modelToggle = screen.getByRole("button", { name: "展开可用模型" });
    expect(modelToggle.disabled).toBe(false);
    await user.click(modelToggle);
    const presetOptions = await screen.findByRole("listbox", { name: "可用模型" });
    expect(within(presetOptions).getByRole("option", { name: "kimi-k3" })).toBeTruthy();
    await user.click(within(presetOptions).getByRole("option", { name: "kimi-k3" }));

    await user.type(screen.getByLabelText("API_KEY"), "secret-for-test");
    await user.click(screen.getByRole("button", { name: "检索可用模型" }));

    const options = await screen.findByRole("listbox", { name: "可用模型" });
    expect(api.listAiModels).toHaveBeenCalledWith(
      expect.objectContaining({
        providerId: "kimi",
        baseUrl: "https://api.moonshot.cn/v1",
        apiKey: "secret-for-test",
      }),
    );
    expect(within(options).queryByRole("option", { name: "kimi-k2.6" })).toBeNull();
    await user.click(within(options).getByRole("option", { name: "kimi-k2.7" }));
    expect(screen.getByLabelText("MODEL_NAME").value).toBe("kimi-k2.7");
  });

  it("ignores the key field in environment mode and reports test latency", async () => {
    const user = userEvent.setup();
    const api = client({
      getAiConfig: vi
        .fn()
        .mockResolvedValue(configuration({ hasEnvironmentApiKey: true })),
    });
    render(<AgentSettings client={api} />);

    const environmentOption = await screen.findByRole("checkbox", {
      name: "从系统环境变量获取",
    });
    const keyInput = screen.getByLabelText("API_KEY");
    await user.type(keyInput, "temporary-key");
    await user.click(environmentOption);
    expect(screen.getByText("获取成功")).toBeTruthy();
    expect(keyInput.disabled).toBe(true);
    expect(keyInput.value).toBe("");

    await user.click(screen.getByRole("button", { name: "测试连接" }));
    expect(await screen.findByText("连接成功 · 86 ms")).toBeTruthy();
    expect(api.testAiConnection).toHaveBeenCalledWith(
      expect.objectContaining({ useEnvironmentKey: true, apiKey: "" }),
    );

    await user.click(
      screen.getByRole("button", {
        name: "查看各供应商 API Key 环境变量名称",
      }),
    );
    const dialog = screen.getByRole("dialog", { name: "API Key 环境变量" });
    expect(within(dialog).getByText("MOONSHOT_API_KEY")).toBeTruthy();
  });

  it("clears the transient key after saving it to the Rust boundary", async () => {
    const user = userEvent.setup();
    const api = client();
    render(<AgentSettings client={api} />);

    const keyInput = await screen.findByLabelText("API_KEY");
    await user.type(keyInput, "new-secret");
    await user.click(screen.getByRole("button", { name: "保存配置" }));

    await waitFor(() => expect(api.saveAiConfig).toHaveBeenCalled());
    expect(keyInput.value).toBe("");
    expect(await screen.findByText("模型配置已保存。")).toBeTruthy();
  });

  it("saves the selected AI reading language using its native label", async () => {
    const user = userEvent.setup();
    const api = client();
    render(<AgentSettings client={api} />);

    const language = await screen.findByRole("combobox", {
      name: "AI 翻译语言",
    });
    expect(language.textContent).toContain("中文（简体）");
    await user.click(language);
    await user.click(screen.getByRole("option", { name: "日本語" }));
    await user.click(screen.getByRole("button", { name: "保存配置" }));

    await waitFor(() =>
      expect(api.saveAiConfig).toHaveBeenCalledWith(
        expect.objectContaining({ translationLanguage: "ja" }),
      ),
    );
  });

  it("shows the compose assistant default as an enabled capsule preference", async () => {
    const user = userEvent.setup();
    const onDefaultAiAssistantOpenChange = vi.fn();
    render(
      <AgentSettings
        client={client()}
        defaultAiAssistantOpen
        onDefaultAiAssistantOpenChange={onDefaultAiAssistantOpenChange}
      />,
    );

    const preference = await screen.findByRole("checkbox", {
      name: /默认开启 AI 助理/,
    });
    expect(preference.checked).toBe(true);
    await user.click(preference);
    expect(onDefaultAiAssistantOpenChange).toHaveBeenCalledWith(false);
  });

  it("automatically saves valid edits while retaining the manual save action", async () => {
    const user = userEvent.setup();
    const api = client();
    render(<AgentSettings client={api} />);

    const language = await screen.findByRole("combobox", {
      name: "AI 翻译语言",
    });
    expect(api.saveAiConfig).not.toHaveBeenCalled();

    await user.click(language);
    await user.click(screen.getByRole("option", { name: "日本語" }));

    await waitFor(
      () =>
        expect(api.saveAiConfig).toHaveBeenCalledWith(
          expect.objectContaining({ translationLanguage: "ja" }),
        ),
      { timeout: 1800 },
    );
    expect(await screen.findByText("配置已自动保存。")).toBeTruthy();

    const manualSave = screen.getByRole("button", { name: "保存配置" });
    expect(manualSave).toBeTruthy();
    await user.click(manualSave);
    await waitFor(() => expect(api.saveAiConfig).toHaveBeenCalledTimes(2));
  });

  it("keeps the settings page visible when configuration loading throws synchronously", async () => {
    const api = client({
      getAiConfig: vi.fn(() => {
        throw new Error("desktop bridge is not ready");
      }),
    });

    render(<AgentSettings client={api} />);

    expect(
      await screen.findByText(/AI 配置读取失败，请重试/),
    ).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Agent 配置" })).toBeTruthy();
  });

  it("contains unexpected render failures inside the Agent settings page", async () => {
    const brokenPresets = new Proxy([], {
      get(target, property, receiver) {
        if (property === "find") throw new Error("unexpected render failure");
        return Reflect.get(target, property, receiver);
      },
    });
    const api = client({
      getAiConfig: vi.fn().mockResolvedValue(
        configuration({
          presets: brokenPresets,
        }),
      ),
    });

    render(<AgentSettings client={api} />);

    expect(
      await screen.findByText("Agent 配置暂时无法显示"),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "重新加载" })).toBeTruthy();
  });
});
