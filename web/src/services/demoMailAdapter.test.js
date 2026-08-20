import { afterEach, describe, expect, it, vi } from "vitest";
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

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

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

describe("demo custom appearance palettes", () => {
  it("uses image analysis only as the initial palette and then remembers the user's choice", async () => {
    class FakeImage {
      naturalWidth = 2;
      naturalHeight = 2;

      set src(_value) {
        queueMicrotask(() => this.onload?.());
      }
    }
    vi.stubGlobal("Image", FakeImage);
    const originalCreateElement = document.createElement.bind(document);
    vi.spyOn(document, "createElement").mockImplementation((tagName, options) => {
      if (tagName !== "canvas") return originalCreateElement(tagName, options);
      return {
        width: 0,
        height: 0,
        getContext: () => ({
          drawImage: () => {},
          getImageData: () => ({
            data: new Uint8ClampedArray([
              35, 190, 150, 255,
              35, 190, 150, 255,
              35, 190, 150, 255,
              35, 190, 150, 255,
            ]),
          }),
        }),
      };
    });

    const adapter = createAdapter();
    const imported = await adapter.importCustomTheme({
      imageDataUrl: "data:image/png;base64,AQID",
    });
    const presetId = imported.activeTheme.id;
    expect(imported.paletteId).toBe("daylight");
    expect(imported.customPresets[0].paletteId).toBe("teal-light");

    const imageMode = await adapter.updateAppearancePreferences({
      minimalModeEnabled: false,
    });
    expect(imageMode.paletteId).toBe("teal-light");

    const chosen = await adapter.updateAppearancePreferences({
      paletteId: "rose-dark",
    });
    expect(chosen.customPresets[0].paletteId).toBe("rose-dark");

    await adapter.updateAppearancePreferences({ minimalModeEnabled: true });
    await adapter.updateAppearancePreferences({ paletteId: "yellow-light" });
    await adapter.selectAppearanceTheme({ kind: "builtin", id: "night" });
    const dormant = await adapter.selectAppearanceTheme({
      kind: "custom",
      id: presetId,
    });
    expect(dormant.paletteId).toBe("yellow-light");

    const restored = await adapter.updateAppearancePreferences({
      minimalModeEnabled: false,
    });
    expect(restored.paletteId).toBe("rose-dark");
  });
});
