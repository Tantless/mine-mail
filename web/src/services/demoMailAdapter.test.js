import { describe, expect, it } from "vitest";
import { createDemoMailAdapter } from "./demoMailAdapter.js";

function createAdapter() {
  return createDemoMailAdapter({
    normalizeSettings: (value) => value,
    normalizeProfileAvatar: (value) => value,
    normalizeContact: (value) => value,
  });
}

async function clearConfiguredProviders(adapter) {
  const registry = await adapter.getAiProviderRegistry();
  for (const provider of registry.providers) {
    await adapter.deleteAiProviderInstance(provider.id);
  }
}

describe("demo AI provider defaults", () => {
  it("selects the first saved provider with a preferred model", async () => {
    const adapter = createAdapter();
    await clearConfiguredProviders(adapter);

    const registry = await adapter.saveAiProviderInstance({
      id: null,
      providerId: "openai",
      name: "OpenAI Official",
      protocolId: "auto",
      baseUrl: "https://api.openai.com/v1",
      modelName: "gpt-5.6-terra",
      useEnvironmentKey: true,
      apiKey: null,
    });

    expect(registry.defaultProviderInstanceId).toBe(registry.providers[0].id);
    expect(registry.providers[0].isDefault).toBe(true);
  });

  it("selects the only provider after model discovery makes it configurable", async () => {
    const adapter = createAdapter();
    await clearConfiguredProviders(adapter);
    const saved = await adapter.saveAiProviderInstance({
      id: null,
      providerId: "custom",
      name: "Internal AI",
      protocolId: "auto",
      baseUrl: "https://ai.example.com/v1",
      modelName: "",
      useEnvironmentKey: true,
      apiKey: null,
      manualContextWindowTokens: 128000,
    });
    expect(saved.defaultProviderInstanceId).toBeNull();

    const result = await adapter.testAiProviderInstance(saved.providers[0].id);
    const registry = await adapter.getAiProviderRegistry();
    expect(result.provider.isDefault).toBe(true);
    expect(registry.defaultProviderInstanceId).toBe(saved.providers[0].id);
  });
});
