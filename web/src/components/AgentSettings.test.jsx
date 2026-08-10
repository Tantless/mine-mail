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
        models: ["gpt-5.6-terra", "gpt-5.6-sol"],
      },
      {
        id: "openai_chat_completions",
        label: "OpenAI Chat Completions",
        baseUrl: "https://api.openai.com/v1",
        recommended: false,
        models: ["gpt-5.6-terra"],
      },
    ],
  },
  {
    id: "kimi",
    label: "Kimi",
    baseUrl: "https://api.moonshot.cn/v1",
    environmentVariable: "MOONSHOT_API_KEY",
    models: ["kimi-k2.6", "kimi-k3"],
    recommendedProtocolId: "openai_chat_completions",
    protocols: [{
      id: "openai_chat_completions",
      label: "OpenAI Chat Completions",
      baseUrl: "https://api.moonshot.cn/v1",
      recommended: true,
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

  it("renders ordered provider details, the active badge, latency, and row actions", async () => {
    const user = userEvent.setup();
    render(<AgentSettings client={client()} />);
    await expandModelConfiguration(user);

    const list = await screen.findByRole("list", { name: "已配置 AI 渠道" });
    expect(within(list).getByText("Work OpenAI")).toBeTruthy();
    expect(within(list).getByText("使用中")).toBeTruthy();
    expect(within(list).getByText(/86 ms · 2 个模型/)).toBeTruthy();
    expect(within(list).getByRole("button", { name: /测试 Work OpenAI/ })).toBeTruthy();
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
      baseUrl: "https://api.moonshot.cn/v1",
      apiKey: "demo-key",
    });
  });

  it("saves before an explicit test and keeps a failed test inside its provider row", async () => {
    const user = userEvent.setup();
    const api = client({
      testAiProviderInstance: vi.fn().mockRejectedValue(new Error("API Key 已失效")),
    });
    render(<AgentSettings client={api} />);
    await expandModelConfiguration(user);

    await user.click(screen.getByRole("button", { name: /测试 Work OpenAI/ }));

    expect(await screen.findByText("API Key 已失效")).toBeTruthy();
    expect(screen.getByText("API Key 已失效").closest(".agent-provider-row")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Agent 配置" })).toBeTruthy();
  });

  it("returns to the saved provider row when save-and-test finds an expired channel", async () => {
    const user = userEvent.setup();
    const api = client({
      testAiProviderInstance: vi.fn().mockRejectedValue(new Error("渠道凭据已过期")),
    });
    render(<AgentSettings client={api} />);
    await expandModelConfiguration(user);
    await user.click(screen.getByRole("button", { name: "编辑 Work OpenAI" }));
    await user.click(screen.getByRole("button", { name: "保存并测试" }));

    await waitFor(() => expect(api.saveAiProviderInstance).toHaveBeenCalledTimes(1));
    expect(api.testAiProviderInstance).toHaveBeenCalledWith(
      "11111111-1111-4111-8111-111111111111",
    );
    const error = await screen.findByText("渠道凭据已过期");
    expect(error.closest(".agent-provider-row")).toBeTruthy();
    expect(screen.queryByRole("heading", { name: /编辑 Work OpenAI/ })).toBeNull();
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
    expect(screen.getByRole("button", { name: "保存并测试" })).toBeTruthy();
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
