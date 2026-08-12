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
    recommendedProtocolId: "openai_chat_completions",
    protocols: [{
      id: "openai_chat_completions",
      label: "OpenAI Chat Completions",
      baseUrl: "",
      recommended: true,
      recommendationRank: 30,
      compatibleModelPrefixes: [],
      recommendedBaseUrlHosts: [],
      maturity: "compatibility",
      models: [],
    }, {
      id: "openai_responses",
      label: "OpenAI Responses",
      baseUrl: "",
      recommended: false,
      recommendationRank: 20,
      compatibleModelPrefixes: [],
      recommendedBaseUrlHosts: ["api.xiaomimimo.com"],
      maturity: "compatibility",
      models: [],
    }],
  },
  {
    id: "openai",
    label: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    environmentVariable: "OPENAI_API_KEY",
    models: ["gpt-5.6-terra", "gpt-5.6-sol"],
    recommendedProtocolId: "openai_responses",
    protocols: [
      {
        id: "openai_responses",
        label: "OpenAI Responses",
        baseUrl: "https://api.openai.com/v1",
        recommended: true,
        recommendationRank: 100,
        compatibleModelPrefixes: [],
        recommendedBaseUrlHosts: [],
        maturity: "stable",
        models: ["gpt-5.6-terra", "gpt-5.6-sol"],
      },
      {
        id: "openai_chat_completions",
        label: "OpenAI Chat Completions",
        baseUrl: "https://api.openai.com/v1",
        recommended: false,
        recommendationRank: 50,
        compatibleModelPrefixes: [],
        recommendedBaseUrlHosts: [],
        maturity: "stable",
        models: ["gpt-5.6-terra"],
      },
    ],
  },
  {
    id: "kimi",
    label: "Kimi",
    baseUrl: "https://api.moonshot.ai/v1",
    environmentVariable: "MOONSHOT_API_KEY",
    models: ["kimi-k2.6", "kimi-k3"],
    recommendedProtocolId: "openai_chat_completions",
    protocols: [{
      id: "openai_chat_completions",
      label: "OpenAI Chat Completions",
      baseUrl: "https://api.moonshot.ai/v1",
      recommended: true,
      recommendationRank: 50,
      compatibleModelPrefixes: [],
      recommendedBaseUrlHosts: [],
      maturity: "stable",
      models: ["kimi-k2.6", "kimi-k3"],
    }],
  },
];

function configuredProvider(overrides = {}) {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    providerId: "openai",
    providerLabel: "OpenAI",
    name: "Work OpenAI",
    protocolId: "auto",
    resolvedProtocolId: "openai_responses",
    protocolLabel: "OpenAI Responses",
    baseUrl: "https://api.openai.com/v1",
    modelName: "gpt-5.6-terra",
    useEnvironmentKey: false,
    hasStoredApiKey: true,
    hasEnvironmentApiKey: false,
    environmentVariable: "OPENAI_API_KEY",
    models: ["gpt-5.6-terra", "gpt-5.6-sol"],
    sortOrder: 0,
    isDefault: true,
    status: "available",
    latencyMs: 86,
    checkedAtMs: 1,
    protocolMaturity: "stable",
    capabilityStatus: "verified",
    capabilityEvidence: "probed",
    structuredOutputStatus: "supported",
    toolCallingStatus: "supported",
    multiTurnToolCallingStatus: "supported",
    ...overrides,
  };
}

function registry(overrides = {}) {
  return {
    providers: [configuredProvider()],
    presets,
    defaultProviderInstanceId: "11111111-1111-4111-8111-111111111111",
    translationLanguage: "zh-Hans",
    translationLanguages: [
      { value: "zh-Hans", label: "中文（简体）" },
      { value: "en", label: "English" },
      { value: "ja", label: "日本語" },
    ],
    ...overrides,
  };
}

function client(overrides = {}) {
  const current = registry();
  return {
    getAiProviderRegistry: vi.fn().mockResolvedValue(current),
    saveAiProviderInstance: vi.fn().mockResolvedValue(current),
    testAiProviderInstance: vi.fn().mockResolvedValue({
      provider: configuredProvider(),
      modelCount: 2,
    }),
    testAiProviderCapabilities: vi.fn().mockResolvedValue({
      provider: configuredProvider(),
      modelCount: 2,
    }),
    setDefaultAiProvider: vi.fn().mockResolvedValue(current),
    deleteAiProviderInstance: vi.fn().mockResolvedValue(
      registry({
        providers: [],
        defaultProviderInstanceId: null,
      }),
    ),
    reorderAiProviderInstances: vi.fn().mockResolvedValue(current),
    setAiTranslationLanguage: vi.fn().mockImplementation(async (languageId) => ({
      translationLanguage: languageId,
      translationLanguages: current.translationLanguages,
    })),
    ...overrides,
  };
}

async function expandModelConfiguration(user) {
  const disclosure = await screen.findByRole("button", { name: /模型配置/ });
  if (disclosure.getAttribute("aria-expanded") !== "true") {
    await user.click(disclosure);
  }
  return disclosure;
}

describe("AgentSettings", () => {
  afterEach(() => cleanup());

  it("keeps the provider manager collapsed and shows the default route summary", async () => {
    render(<AgentSettings client={client()} />);

    const disclosure = await screen.findByRole("button", { name: /模型配置/ });
    expect(disclosure.getAttribute("aria-expanded")).toBe("false");
    expect(disclosure.textContent).toContain("Work OpenAI · gpt-5.6-terra");
    expect(screen.getByRole("combobox", { name: "AI 翻译语言" })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: /默认开启 AI 助理/ })).toBeTruthy();
  });

  it("keeps protocol, capability, and test actions out of the provider list", async () => {
    const user = userEvent.setup();
    render(<AgentSettings client={client()} />);
    await expandModelConfiguration(user);

    const list = await screen.findByRole("list", { name: "已配置 AI 渠道" });
    expect(within(list).getByText("Work OpenAI")).toBeTruthy();
    expect(within(list).getByText("使用中")).toBeTruthy();
    expect(within(list).getByText(/86 ms · 2 个模型/)).toBeTruthy();
    expect(within(list).queryByText("OpenAI Responses")).toBeNull();
    expect(within(list).queryByText("能力已验证")).toBeNull();
    expect(within(list).getByRole("button", { name: "测试 Work OpenAI 连接" })).toBeTruthy();
    expect(within(list).getByRole("button", { name: "编辑 Work OpenAI" })).toBeTruthy();
    expect(within(list).getByRole("button", { name: "删除 Work OpenAI" })).toBeTruthy();
  });

  it("adds another instance through the searchable preset and detail flow", async () => {
    const user = userEvent.setup();
    const api = client();
    render(<AgentSettings client={api} />);
    await expandModelConfiguration(user);

    await user.click(screen.getByRole("button", { name: "添加 AI 渠道" }));
    await user.type(screen.getByPlaceholderText("搜索渠道"), "Kimi");
    await user.click(screen.getByRole("listitem", { name: /Kimi/ }));

    const name = screen.getByLabelText("渠道名称");
    await user.clear(name);
    await user.type(name, "Kimi Backup");
    await user.type(screen.getByLabelText("API_KEY"), "demo-key");
    await user.click(screen.getByRole("button", { name: "保存渠道" }));

    await waitFor(() => expect(api.saveAiProviderInstance).toHaveBeenCalledTimes(1));
    expect(api.saveAiProviderInstance.mock.calls[0][0]).toMatchObject({
      id: null,
      providerId: "kimi",
      name: "Kimi Backup",
      baseUrl: "https://api.moonshot.ai/v1",
      apiKey: "demo-key",
    });
  });

  it("recommends Responses when a custom automatic channel uses an official MiMo endpoint", async () => {
    const user = userEvent.setup();
    render(<AgentSettings client={client()} />);
    await expandModelConfiguration(user);

    await user.click(screen.getByRole("button", { name: "添加 AI 渠道" }));
    await user.type(screen.getByPlaceholderText("搜索渠道"), "自定义");
    await user.click(screen.getByRole("listitem", { name: /自定义/ }));
    const baseUrl = screen.getByLabelText("BASE_URL");
    await user.type(baseUrl, "https://api.xiaomimimo.com/v1");

    expect(screen.getByRole("combobox", { name: "API 协议" }).textContent).toContain(
      "自动（当前使用：OpenAI Responses）",
    );
  });

  it("switches the automatic DeepSeek route by model and blocks an incompatible explicit route", async () => {
    const user = userEvent.setup();
    const deepseek = {
      id: "deepseek",
      label: "DeepSeek",
      baseUrl: "https://api.deepseek.com",
      environmentVariable: "DEEPSEEK_API_KEY",
      models: ["deepseek-v4-flash", "deepseek-v4-pro"],
      recommendedProtocolId: "openai_responses",
      protocols: [{
        id: "openai_responses",
        label: "OpenAI Responses",
        baseUrl: "https://api.deepseek.com",
        recommended: true,
        recommendationRank: 100,
        compatibleModelPrefixes: ["deepseek-v4-flash"],
        recommendedBaseUrlHosts: [],
        maturity: "stable",
        limitation: "当前仅 DeepSeek V4 Flash 支持",
        models: ["deepseek-v4-flash", "deepseek-v4-pro"],
      }, {
        id: "openai_chat_completions",
        label: "OpenAI Chat Completions",
        baseUrl: "https://api.deepseek.com",
        recommended: false,
        recommendationRank: 50,
        compatibleModelPrefixes: [],
        recommendedBaseUrlHosts: [],
        maturity: "stable",
        models: ["deepseek-v4-flash", "deepseek-v4-pro"],
      }],
    };
    render(<AgentSettings client={client({
      getAiProviderRegistry: vi.fn().mockResolvedValue(registry({ presets: [...presets, deepseek] })),
    })} />);
    await expandModelConfiguration(user);
    await user.click(screen.getByRole("button", { name: "添加 AI 渠道" }));
    await user.type(screen.getByPlaceholderText("搜索渠道"), "DeepSeek");
    await user.click(screen.getByRole("listitem", { name: /DeepSeek/ }));

    expect(screen.getByRole("combobox", { name: "API 协议" }).textContent).toContain(
      "自动（当前使用：OpenAI Responses）",
    );
    await user.clear(screen.getByLabelText("首选模型"));
    await user.type(screen.getByLabelText("首选模型"), "deepseek-v4-pro");
    expect(screen.getByRole("combobox", { name: "API 协议" }).textContent).toContain(
      "自动（当前使用：OpenAI Chat Completions）",
    );

    await user.click(screen.getByRole("combobox", { name: "API 协议" }));
    expect(screen.getByRole("option", { name: /OpenAI Responses/ }).disabled).toBe(true);
  });

  it("persists the manually selected context window for a custom channel", async () => {
    const user = userEvent.setup();
    const api = client();
    render(<AgentSettings client={api} />);
    await expandModelConfiguration(user);

    await user.click(screen.getByRole("button", { name: "添加 AI 渠道" }));
    await user.type(screen.getByPlaceholderText("搜索渠道"), "自定义");
    await user.click(screen.getByRole("listitem", { name: /自定义/ }));
    await user.type(screen.getByLabelText("渠道名称"), "内部模型");
    await user.type(screen.getByLabelText("BASE_URL"), "https://ai.example.com/v1");
    await user.type(screen.getByLabelText("API_KEY"), "demo-key");
    await user.type(screen.getByLabelText("首选模型"), "internal-model");
    await user.click(screen.getByRole("combobox", { name: "上下文窗口" }));
    await user.click(screen.getByRole("option", { name: "500K" }));
    await user.click(screen.getByRole("button", { name: "保存渠道" }));

    await waitFor(() => expect(api.saveAiProviderInstance).toHaveBeenCalledTimes(1));
    expect(api.saveAiProviderInstance.mock.calls[0][0]).toMatchObject({
      providerId: "custom",
      manualContextWindowTokens: 500000,
    });
  });

  it("keeps a failed connection test inside the provider editor", async () => {
    const user = userEvent.setup();
    const api = client({
      testAiProviderInstance: vi.fn().mockRejectedValue(new Error("API Key 已失效")),
    });
    render(<AgentSettings client={api} />);
    await expandModelConfiguration(user);

    await user.click(screen.getByRole("button", { name: "编辑 Work OpenAI" }));
    await user.click(screen.getByRole("button", { name: "测试连接" }));

    expect(await screen.findByText("API Key 已失效")).toBeTruthy();
    expect(screen.getByText("API Key 已失效").closest(".agent-provider-flow")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Agent 配置" })).toBeTruthy();
  });

  it("tests only connectivity from the provider list action", async () => {
    const user = userEvent.setup();
    const api = client();
    render(<AgentSettings client={api} />);
    await expandModelConfiguration(user);

    await user.click(screen.getByRole("button", { name: "测试 Work OpenAI 连接" }));

    await waitFor(() => expect(api.testAiProviderInstance).toHaveBeenCalledWith(
      "11111111-1111-4111-8111-111111111111",
    ));
    expect(api.testAiProviderCapabilities).not.toHaveBeenCalled();
    expect(api.saveAiProviderInstance).not.toHaveBeenCalled();
    expect(screen.getByRole("list", { name: "已配置 AI 渠道" })).toBeTruthy();
  });

  it("stays in the editor when a connection test finds an expired channel", async () => {
    const user = userEvent.setup();
    const api = client({
      testAiProviderInstance: vi.fn().mockRejectedValue(new Error("渠道凭据已过期")),
    });
    render(<AgentSettings client={api} />);
    await expandModelConfiguration(user);
    await user.click(screen.getByRole("button", { name: "编辑 Work OpenAI" }));
    await user.click(screen.getByRole("button", { name: "测试连接" }));

    await waitFor(() => expect(api.saveAiProviderInstance).toHaveBeenCalledTimes(1));
    expect(api.testAiProviderInstance).toHaveBeenCalledWith(
      "11111111-1111-4111-8111-111111111111",
    );
    const error = await screen.findByText("渠道凭据已过期");
    expect(error.closest(".agent-provider-flow")).toBeTruthy();
    expect(screen.getByText(/编辑 Work OpenAI/)).toBeTruthy();
  });

  it("runs capability testing separately and shows its detail only in the editor", async () => {
    const user = userEvent.setup();
    const api = client();
    render(<AgentSettings client={api} />);
    await expandModelConfiguration(user);
    await user.click(screen.getByRole("button", { name: "编辑 Work OpenAI" }));

    expect(screen.getByRole("region", { name: "连接与能力" })).toBeTruthy();
    expect(screen.getByText("结构化输出")).toBeTruthy();
    expect(screen.getByText("多轮工具续接")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "测试能力" }));

    await waitFor(() => expect(api.testAiProviderCapabilities).toHaveBeenCalledWith(
      "11111111-1111-4111-8111-111111111111",
    ));
    expect(api.testAiProviderInstance).not.toHaveBeenCalled();
    expect(await screen.findByText(/能力测试完成/)).toBeTruthy();
  });

  it("edits a provider without reading the stored key back into React", async () => {
    const user = userEvent.setup();
    render(<AgentSettings client={client()} />);
    await expandModelConfiguration(user);
    await user.click(screen.getByRole("button", { name: "编辑 Work OpenAI" }));

    const key = screen.getByLabelText("API_KEY");
    expect(key.value).toBe("••••••••••••");
    await user.click(key);
    expect(key.value).toBe("");
    expect(screen.getByRole("button", { name: "测试连接" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "测试能力" })).toBeTruthy();
  });

  it("requires the shared confirmation before deleting a default provider", async () => {
    const user = userEvent.setup();
    const api = client();
    render(<AgentSettings client={api} />);
    await expandModelConfiguration(user);
    await user.click(screen.getByRole("button", { name: "删除 Work OpenAI" }));

    const dialog = screen.getByRole("alertdialog", { name: "删除 AI 渠道？" });
    expect(within(dialog).getByText(/默认项会清空/)).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "删除渠道" }));
    await waitFor(() =>
      expect(api.deleteAiProviderInstance).toHaveBeenCalledWith(
        "11111111-1111-4111-8111-111111111111",
      ));
  });

  it("saves the translation preference independently of provider management", async () => {
    const user = userEvent.setup();
    const api = client();
    render(<AgentSettings client={api} />);

    await user.click(await screen.findByRole("combobox", { name: "AI 翻译语言" }));
    await user.click(screen.getByRole("option", { name: "日本語" }));
    await waitFor(() =>
      expect(api.setAiTranslationLanguage).toHaveBeenCalledWith("ja"));
  });
});
