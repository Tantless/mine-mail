import { Component, useMemo, useRef, useState, useEffect } from "react";
import {
  ArrowLeft,
  CaretDown,
  Check,
  DotsSixVertical,
  MagnifyingGlass,
  PencilSimple,
  Plus,
  Pulse,
  Question,
  SpinnerGap,
  Trash,
  X,
} from "@phosphor-icons/react";
import { mailApi } from "../services/mailApi.js";
import { userFacingErrorMessage } from "../utils/userFacingError.js";
import { limitText, textInputLimits } from "../utils/textLimits.js";
import { ConsequentialConfirmDialog } from "./ConsequentialConfirmDialog.jsx";
import { useConfirmDialogFocus } from "./ConfirmDialogPrimitives.jsx";
import { IconButton } from "./IconButton.jsx";
import { ProfileAvatar } from "./ProfileAvatar.jsx";
import { ThemedSelect } from "./ThemedSelect.jsx";

const fallbackTranslationLanguages = Object.freeze([
  { value: "zh-Hans", label: "中文（简体）" },
  { value: "zh-Hant", label: "中文（繁體）" },
  { value: "en", label: "English" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
  { value: "ru", label: "Русский" },
  { value: "es", label: "Español" },
  { value: "fr", label: "Français" },
  { value: "de", label: "Deutsch" },
  { value: "pt", label: "Português" },
  { value: "it", label: "Italiano" },
  { value: "ar", label: "العربية" },
]);

const providerIconDomains = Object.freeze({
  openai: "openai.com",
  anthropic: "anthropic.com",
  deepseek: "deepseek.com",
  kimi: "moonshot.cn",
  qwen: "qwen.ai",
  mimo: "xiaomi.com",
  minimax: "minimax.io",
  modelscope: "modelscope.cn",
  doubaoseed: "bytedance.com",
  glm: "bigmodel.cn",
  openrouter: "openrouter.ai",
});

const storedApiKeyMask = "••••••••••••";

function emptyRegistry() {
  return {
    providers: [],
    presets: [],
    defaultProviderInstanceId: null,
    translationLanguage: "zh-Hans",
    translationLanguages: fallbackTranslationLanguages,
    contextWindowOptions: [128000, 200000, 500000, 1000000, 2000000],
  };
}

function normalizeRegistry(registry) {
  const value = registry && typeof registry === "object" ? registry : {};
  return {
    ...emptyRegistry(),
    ...value,
    providers: Array.isArray(value.providers) ? value.providers : [],
    presets: Array.isArray(value.presets) ? value.presets : [],
    translationLanguages:
      Array.isArray(value.translationLanguages) && value.translationLanguages.length
        ? value.translationLanguages
        : fallbackTranslationLanguages,
  };
}

function legacyRegistry(config) {
  const provider = config?.baseUrl && config?.modelName
    ? {
        id: `legacy-${config.providerId}`,
        providerId: config.providerId,
        providerLabel:
          config.presets?.find((preset) => preset.id === config.providerId)?.label
          || "自定义",
        name:
          config.presets?.find((preset) => preset.id === config.providerId)?.label
          || "默认渠道",
        protocolId: config.protocolId,
        resolvedProtocolId: config.resolvedProtocolId,
        protocolLabel:
          config.presets
            ?.find((preset) => preset.id === config.providerId)
            ?.protocols?.find((protocol) => protocol.id === config.resolvedProtocolId)
            ?.label || "OpenAI Chat Completions",
        baseUrl: config.baseUrl,
        modelName: config.modelName,
        useEnvironmentKey: config.useEnvironmentKey,
        hasStoredApiKey: config.hasStoredApiKey,
        hasEnvironmentApiKey: config.hasEnvironmentApiKey,
        environmentVariable: config.environmentVariable,
        models:
          config.presets?.find((preset) => preset.id === config.providerId)?.models || [],
        sortOrder: 0,
        isDefault: true,
        status: "untested",
        latencyMs: null,
      }
    : null;
  return normalizeRegistry({
    providers: provider ? [provider] : [],
    presets: config?.presets || [],
    defaultProviderInstanceId: provider?.id || null,
    translationLanguage: config?.translationLanguage || "zh-Hans",
    translationLanguages: config?.translationLanguages || fallbackTranslationLanguages,
    contextWindowOptions: [128000, 200000, 500000, 1000000, 2000000],
  });
}

function providerDomain(providerId, baseUrl = "") {
  if (providerIconDomains[providerId]) return providerIconDomains[providerId];
  try {
    return new URL(baseUrl).hostname || "provider.local";
  } catch {
    return "provider.local";
  }
}

function ProviderMark({ providerId, baseUrl, label, className = "" }) {
  return (
    <ProfileAvatar
      className={`agent-provider-mark ${className}`.trim()}
      email={`agent@${providerDomain(providerId, baseUrl)}`}
      label={label}
    />
  );
}

function protocolSupportsModel(protocol, modelName) {
  const prefixes = protocol?.compatibleModelPrefixes || [];
  const normalized = String(modelName || "").trim().toLowerCase();
  return !normalized || !prefixes.length || prefixes.some(
    (prefix) => normalized.startsWith(String(prefix).toLowerCase()),
  );
}

function protocolRecommendationRank(protocol, baseUrl) {
  let score = Number(protocol?.recommendationRank || 0);
  try {
    const host = new URL(baseUrl || "").hostname.toLowerCase();
    if ((protocol?.recommendedBaseUrlHosts || []).some(
      (candidate) => String(candidate).toLowerCase() === host,
    )) score += 100;
  } catch {
    // An incomplete custom URL cannot supply a host-specific recommendation.
  }
  return score;
}

function recommendedProtocolId(preset, form) {
  const compatible = (preset?.protocols || [])
    .filter((protocol) => protocolSupportsModel(protocol, form?.modelName));
  if (!compatible.length) return preset?.recommendedProtocolId;
  return compatible.reduce((best, protocol) => (
    protocolRecommendationRank(protocol, form?.baseUrl)
      > protocolRecommendationRank(best, form?.baseUrl)
      ? protocol
      : best
  )).id;
}

function protocolOptions(preset, form) {
  if (!preset) return [];
  const recommendation = recommendedProtocolId(preset, form);
  const recommended = preset.protocols?.find(
    (protocol) => protocol.id === recommendation,
  );
  return [
    {
      value: "auto",
      label: `自动（当前使用：${recommended?.label || "渠道默认"}）`,
    },
    ...(preset.protocols || []).map((protocol) => ({
      value: protocol.id,
      disabled: !protocolSupportsModel(protocol, form?.modelName),
      label: [
        protocol.label,
        protocol.maturity === "beta" ? "Beta" : null,
        protocol.id === recommendation ? "推荐" : null,
      ].filter(Boolean).join(" · "),
    })),
  ];
}

function resolvedProtocol(preset, form) {
  const protocolId = form?.protocolId === "auto"
    ? recommendedProtocolId(preset, form)
    : form?.protocolId;
  return preset?.protocols?.find((protocol) => protocol.id === protocolId) || null;
}

function routeCapabilityCopy(provider) {
  const status = provider.capabilityStatus || "untested";
  if (status === "verified") return "能力已验证";
  if (status === "limited") return "部分能力受限";
  if (status === "unstable") return "能力验证不稳定";
  if (status === "stale") return "能力验证已过期";
  if (status === "tested") return "连接已验证";
  return "能力未测试";
}

function capabilitySupportCopy(status) {
  if (status === "supported") return "支持";
  if (status === "unsupported") return "不支持";
  if (status === "unstable") return "验证不稳定";
  return "未验证";
}

function providerForm(preset, provider = null) {
  const protocolId = provider?.protocolId || "auto";
  const resolvedProtocolId = protocolId === "auto"
    ? recommendedProtocolId(preset, provider)
    : protocolId;
  const protocol = preset.protocols?.find(
    (candidate) => candidate.id === resolvedProtocolId,
  );
  return {
    id: provider?.id || null,
    providerId: preset.id,
    name: provider?.name || preset.label,
    protocolId,
    baseUrl: provider?.baseUrl || protocol?.baseUrl || preset.baseUrl || "",
    modelName:
      provider?.modelName || protocol?.models?.[0] || preset.models?.[0] || "",
    apiKey: "",
    useEnvironmentKey: provider?.useEnvironmentKey || false,
    hasStoredApiKey: provider?.hasStoredApiKey || false,
    hasEnvironmentApiKey: provider?.hasEnvironmentApiKey || false,
    environmentVariable: preset.environmentVariable || "AI_API_KEY",
    models: provider?.models || [],
    manualContextWindowTokens: provider?.manualContextWindowTokens || 128000,
    baseUrlCustomized: Boolean(
      provider
      && !(preset.protocols || []).some(
        (candidate) => candidate.baseUrl === provider.baseUrl,
      ),
    ),
  };
}

const contextWindowLabels = new Map([
  [128000, "128K"],
  [200000, "200K"],
  [500000, "500K"],
  [1000000, "1M"],
  [2000000, "2M"],
]);

function canSaveProvider(form) {
  if (!form?.name?.trim() || !form?.baseUrl?.trim()) return false;
  try {
    const url = new URL(form.baseUrl.trim());
    if (url.protocol !== "https:" && url.protocol !== "http:") return false;
  } catch {
    return false;
  }
  return Boolean(
    form.useEnvironmentKey || form.hasStoredApiKey || form.apiKey?.trim(),
  );
}

function statusCopy(provider) {
  if (provider.status === "available") {
    return provider.latencyMs === null
      ? `可用 · ${provider.models.length} 个模型`
      : `可用 · ${provider.latencyMs} ms · ${provider.models.length} 个模型`;
  }
  if (provider.status === "unavailable") return "连接不可用，请检查此渠道";
  return "尚未测试连接";
}

function AgentSettingsContent({
  client,
  defaultAiAssistantOpen,
  onDefaultAiAssistantOpenChange,
  children,
}) {
  const [expanded, setExpanded] = useState(false);
  const [loadState, setLoadState] = useState("loading");
  const [registry, setRegistry] = useState(emptyRegistry);
  const [view, setView] = useState("list");
  const [presetSearch, setPresetSearch] = useState("");
  const [form, setForm] = useState(null);
  const [editingStoredApiKey, setEditingStoredApiKey] = useState(false);
  const [actionState, setActionState] = useState("idle");
  const [flowFeedback, setFlowFeedback] = useState(null);
  const [providerErrors, setProviderErrors] = useState({});
  const [deleteTarget, setDeleteTarget] = useState(null);
  const [deleteError, setDeleteError] = useState(null);
  const [draggingId, setDraggingId] = useState(null);
  const [translationState, setTranslationState] = useState("idle");
  const [translationFeedback, setTranslationFeedback] = useState(null);
  const [helpOpen, setHelpOpen] = useState(false);
  const helpCloseRef = useRef(null);
  const helpFocus = useConfirmDialogFocus({
    open: helpOpen,
    initialFocusRef: helpCloseRef,
    onCancel: () => setHelpOpen(false),
  });

  const loadRegistry = async () => {
    if (typeof client?.getAiProviderRegistry === "function") {
      return normalizeRegistry(await client.getAiProviderRegistry());
    }
    if (typeof client?.getAiConfig === "function") {
      return legacyRegistry(await client.getAiConfig());
    }
    throw new Error("AI provider registry client is unavailable");
  };

  useEffect(() => {
    let cancelled = false;
    setLoadState("loading");
    loadRegistry()
      .then((next) => {
        if (cancelled) return;
        setRegistry(next);
        setLoadState("ready");
      })
      .catch(() => {
        if (!cancelled) setLoadState("error");
      });
    return () => {
      cancelled = true;
    };
  }, [client]);

  const defaultProvider = registry.providers.find((provider) => provider.isDefault);
  const selectedPreset = useMemo(
    () => registry.presets.find((preset) => preset.id === form?.providerId) || null,
    [form?.providerId, registry.presets],
  );
  const filteredPresets = useMemo(() => {
    const query = presetSearch.trim().toLocaleLowerCase();
    if (!query) return registry.presets;
    return registry.presets.filter((preset) =>
      `${preset.label} ${preset.id}`.toLocaleLowerCase().includes(query));
  }, [presetSearch, registry.presets]);
  const busy = actionState !== "idle";

  const openAddFlow = () => {
    setPresetSearch("");
    setForm(null);
    setFlowFeedback(null);
    setView("presets");
  };

  const openEditFlow = (provider) => {
    const preset = registry.presets.find(
      (candidate) => candidate.id === provider.providerId,
    );
    if (!preset) return;
    setForm(providerForm(preset, provider));
    setEditingStoredApiKey(false);
    setFlowFeedback(null);
    setView("edit");
  };

  const choosePreset = (preset) => {
    setForm(providerForm(preset));
    setEditingStoredApiKey(false);
    setFlowFeedback(null);
    setView("edit");
  };

  const saveProvider = async ({ testKind = null } = {}) => {
    if (!canSaveProvider(form)) {
      setFlowFeedback({
        tone: "danger",
        text: "请完整填写渠道名称、地址和 API Key。",
      });
      return;
    }
    if (typeof client?.saveAiProviderInstance !== "function") {
      setFlowFeedback({ tone: "danger", text: "当前客户端不支持多渠道配置。" });
      return;
    }
    setActionState(testKind ? `saving-for-${testKind}` : "saving");
    setFlowFeedback(null);
    try {
      let next = normalizeRegistry(await client.saveAiProviderInstance(form));
      const saved = next.providers.find(
        (provider) => provider.id === form.id,
      ) || next.providers.find(
        (provider) => provider.name === form.name.trim()
          && provider.providerId === form.providerId,
      );
      if (testKind && saved) {
        setActionState(`testing-${testKind}`);
        try {
          const result = testKind === "capabilities"
            ? await client.testAiProviderCapabilities(saved.id)
            : await client.testAiProviderInstance(saved.id);
          next = await loadRegistry();
          const tested = next.providers.find((provider) => provider.id === saved.id)
            || result?.provider
            || saved;
          setRegistry(next);
          setForm(providerForm(selectedPreset, tested));
          setEditingStoredApiKey(false);
          setFlowFeedback({
            tone: "success",
            text: testKind === "capabilities"
              ? `能力测试完成：${routeCapabilityCopy(tested)}。`
              : `连接测试完成：${statusCopy(tested)}。`,
          });
          setView("edit");
          return;
        } catch (error) {
          next = await loadRegistry().catch(() => next);
          const failed = next.providers.find((provider) => provider.id === saved.id) || saved;
          setRegistry(next);
          setForm(providerForm(selectedPreset, failed));
          setEditingStoredApiKey(false);
          setFlowFeedback({
            tone: "danger",
            text: userFacingErrorMessage(
              error,
              testKind === "capabilities"
                ? "能力测试失败，请检查当前协议和模型。"
                : "连接失败，请检查当前渠道配置。",
            ),
          });
          setView("edit");
          return;
        }
      }
      setRegistry(next);
      setView("list");
      setForm(null);
      setFlowFeedback(null);
    } catch (error) {
      setFlowFeedback({
        tone: "danger",
        text: userFacingErrorMessage(error, "渠道配置保存失败，请检查后重试。"),
      });
    } finally {
      setActionState("idle");
    }
  };

  const setDefaultProvider = async (provider) => {
    if (provider.isDefault || typeof client?.setDefaultAiProvider !== "function") return;
    setActionState(`default:${provider.id}`);
    setProviderErrors((current) => ({ ...current, [provider.id]: null }));
    try {
      setRegistry(normalizeRegistry(await client.setDefaultAiProvider(provider.id)));
    } catch (error) {
      setProviderErrors((current) => ({
        ...current,
        [provider.id]: userFacingErrorMessage(
          error,
          "默认模型设置失败，请检查该渠道。",
        ),
      }));
    } finally {
      setActionState("idle");
    }
  };

  const testProviderConnection = async (provider) => {
    if (typeof client?.testAiProviderInstance !== "function") return;
    setActionState(`testing-connection:${provider.id}`);
    setProviderErrors((current) => ({ ...current, [provider.id]: null }));
    try {
      await client.testAiProviderInstance(provider.id);
      setRegistry(await loadRegistry());
    } catch (error) {
      setRegistry(await loadRegistry().catch(() => registry));
      setProviderErrors((current) => ({
        ...current,
        [provider.id]: userFacingErrorMessage(
          error,
          "连接失败，请编辑并检查该渠道。",
        ),
      }));
    } finally {
      setActionState("idle");
    }
  };

  const confirmDelete = async () => {
    if (!deleteTarget || typeof client?.deleteAiProviderInstance !== "function") return;
    setActionState(`deleting:${deleteTarget.id}`);
    setDeleteError(null);
    try {
      setRegistry(
        normalizeRegistry(await client.deleteAiProviderInstance(deleteTarget.id)),
      );
      setDeleteTarget(null);
    } catch (error) {
      setDeleteError(error);
    } finally {
      setActionState("idle");
    }
  };

  const moveProvider = async (draggedId, targetId) => {
    if (!draggedId || draggedId === targetId) return;
    const providers = [...registry.providers];
    const from = providers.findIndex((provider) => provider.id === draggedId);
    const to = providers.findIndex((provider) => provider.id === targetId);
    if (from < 0 || to < 0) return;
    const [dragged] = providers.splice(from, 1);
    providers.splice(to, 0, dragged);
    const optimistic = providers.map((provider, index) => ({
      ...provider,
      sortOrder: index,
    }));
    setRegistry((current) => ({ ...current, providers: optimistic }));
    setDraggingId(null);
    try {
      if (typeof client?.reorderAiProviderInstances !== "function") {
        throw new Error("当前客户端不支持渠道排序");
      }
      setRegistry(
        normalizeRegistry(
          await client.reorderAiProviderInstances(
            optimistic.map((provider) => provider.id),
          ),
        ),
      );
    } catch (error) {
      setRegistry((current) => ({ ...current, providers: registry.providers }));
      setProviderErrors((current) => ({
        ...current,
        [draggedId]: userFacingErrorMessage(error, "渠道排序保存失败，请重试。"),
      }));
    }
  };

  const updateTranslationLanguage = async (languageId) => {
    if (
      languageId === registry.translationLanguage
      || translationState === "saving"
    ) return;
    const previous = registry.translationLanguage;
    setRegistry((current) => ({ ...current, translationLanguage: languageId }));
    setTranslationState("saving");
    setTranslationFeedback({ tone: "neutral", text: "正在保存翻译语言…" });
    try {
      const saved = await client.setAiTranslationLanguage(languageId);
      setRegistry((current) => ({
        ...current,
        translationLanguage: saved.translationLanguage,
        translationLanguages:
          saved.translationLanguages?.length
            ? saved.translationLanguages
            : current.translationLanguages,
      }));
      setTranslationFeedback({ tone: "success", text: "翻译语言已保存。" });
    } catch (error) {
      setRegistry((current) => ({
        ...current,
        translationLanguage:
          current.translationLanguage === languageId ? previous : current.translationLanguage,
      }));
      setTranslationFeedback({
        tone: "danger",
        text: userFacingErrorMessage(error, "翻译语言保存失败，请重试。"),
      });
    } finally {
      setTranslationState("idle");
    }
  };

  const renderProviderList = () => (
    <>
      <div className="agent-provider-toolbar">
        <span>
          <strong>{registry.providers.length} 个渠道</strong>
          <small>拖动调整同名模型的渠道优先级。</small>
        </span>
        <IconButton
          className="agent-provider-add"
          label="添加 AI 渠道"
          onClick={openAddFlow}
        >
          <Plus size={18} weight="bold" />
        </IconButton>
      </div>

      {registry.providers.length ? (
        <div className="agent-provider-list" role="list" aria-label="已配置 AI 渠道">
          {registry.providers.map((provider) => (
            <article
              key={provider.id}
              className="agent-provider-row"
              data-default={provider.isDefault || undefined}
              data-status={provider.status}
              data-dragging={draggingId === provider.id || undefined}
              role="listitem"
              draggable={!busy}
              onDragStart={(event) => {
                setDraggingId(provider.id);
                event.dataTransfer.effectAllowed = "move";
                event.dataTransfer.setData("text/plain", provider.id);
              }}
              onDragEnd={() => setDraggingId(null)}
              onDragOver={(event) => {
                if (draggingId && draggingId !== provider.id) {
                  event.preventDefault();
                  event.dataTransfer.dropEffect = "move";
                }
              }}
              onDrop={(event) => {
                event.preventDefault();
                void moveProvider(
                  event.dataTransfer.getData("text/plain") || draggingId,
                  provider.id,
                );
              }}
            >
              <span className="agent-provider-drag" aria-hidden="true">
                <DotsSixVertical size={18} weight="bold" />
              </span>
              <ProviderMark
                providerId={provider.providerId}
                baseUrl={provider.baseUrl}
                label={provider.providerLabel}
              />
              <span className="agent-provider-main">
                <span className="agent-provider-title">
                  <strong>{provider.name}</strong>
                  <small>{provider.providerLabel}</small>
                  {provider.isDefault ? (
                    <span className="agent-provider-default-badge">
                      <Check size={12} weight="bold" />
                      使用中
                    </span>
                  ) : null}
                </span>
                <span className="agent-provider-url" title={provider.baseUrl}>
                  {provider.baseUrl}
                </span>
                <span className="agent-provider-meta">
                  <small data-status={provider.status}>{statusCopy(provider)}</small>
                  {provider.modelName ? <small>首选：{provider.modelName}</small> : null}
                </span>
                {providerErrors[provider.id] ? (
                  <small className="agent-provider-inline-error" role="alert">
                    {providerErrors[provider.id]}
                  </small>
                ) : null}
              </span>
              <span className="agent-provider-actions">
                {!provider.isDefault ? (
                  <button
                    type="button"
                    className="agent-provider-use"
                    disabled={busy || !provider.modelName}
                    onClick={() => void setDefaultProvider(provider)}
                  >
                    {actionState === `default:${provider.id}` ? (
                      <SpinnerGap size={14} className="spin" />
                    ) : null}
                    设为默认
                  </button>
                ) : null}
                <IconButton
                  label={`测试 ${provider.name} 连接`}
                  disabled={busy}
                  onClick={() => void testProviderConnection(provider)}
                >
                  {actionState === `testing-connection:${provider.id}` ? (
                    <SpinnerGap size={17} className="spin" />
                  ) : (
                    <Pulse size={17} />
                  )}
                </IconButton>
                <IconButton
                  label={`编辑 ${provider.name}`}
                  disabled={busy}
                  onClick={() => openEditFlow(provider)}
                >
                  <PencilSimple size={17} />
                </IconButton>
                <IconButton
                  label={`删除 ${provider.name}`}
                  tone="danger"
                  disabled={busy}
                  onClick={() => {
                    setDeleteError(null);
                    setDeleteTarget(provider);
                  }}
                >
                  <Trash size={17} />
                </IconButton>
              </span>
            </article>
          ))}
        </div>
      ) : (
        <div className="agent-provider-empty">
          <ProviderMark providerId="custom" label="AI 渠道" />
          <span>
            <strong>还没有配置 AI 渠道</strong>
            <small>添加一个服务后即可测试连接并获取模型。</small>
          </span>
          <button type="button" className="send-button" onClick={openAddFlow}>
            <Plus size={16} weight="bold" />
            添加渠道
          </button>
        </div>
      )}
    </>
  );

  const renderPresetPicker = () => (
    <section className="agent-provider-flow" aria-labelledby="agent-provider-picker-title">
      <header className="agent-provider-flow__heading">
        <IconButton label="返回渠道列表" onClick={() => setView("list")}>
          <ArrowLeft size={18} />
        </IconButton>
        <span>
          <strong id="agent-provider-picker-title">添加新渠道</strong>
          <small>选择预设后填写 API Key 与连接信息。</small>
        </span>
      </header>
      <label className="agent-provider-search">
        <MagnifyingGlass size={16} aria-hidden="true" />
        <input
          value={presetSearch}
          maxLength={textInputLimits.providerSearch}
          autoFocus
          placeholder="搜索渠道"
          onChange={(event) =>
            setPresetSearch(
              limitText(event.target.value, textInputLimits.providerSearch),
            )
          }
        />
      </label>
      <div className="agent-provider-preset-grid" role="list" aria-label="可添加渠道">
        {filteredPresets.map((preset) => (
          <button
            key={preset.id}
            type="button"
            role="listitem"
            aria-label={preset.label}
            onClick={() => choosePreset(preset)}
          >
            <ProviderMark
              providerId={preset.id}
              baseUrl={preset.baseUrl}
              label={preset.label}
            />
            <span>{preset.label}</span>
          </button>
        ))}
      </div>
    </section>
  );

  const renderProviderEditor = () => {
    if (!form || !selectedPreset) return null;
    const options = protocolOptions(selectedPreset, form);
    const route = resolvedProtocol(selectedPreset, form);
    const explicitIncompatible = form.protocolId !== "auto"
      && !protocolSupportsModel(route, form.modelName);
    const editingProvider = form.id
      ? registry.providers.find((provider) => provider.id === form.id) || null
      : null;
    return (
      <section className="agent-provider-flow" aria-labelledby="agent-provider-editor-title">
        <header className="agent-provider-flow__heading">
          <IconButton
            label={form.id ? "返回渠道列表" : "返回选择渠道"}
            disabled={busy}
            onClick={() => setView(form.id ? "list" : "presets")}
          >
            <ArrowLeft size={18} />
          </IconButton>
          <ProviderMark
            providerId={selectedPreset.id}
            baseUrl={form.baseUrl}
            label={selectedPreset.label}
          />
          <span>
            <strong id="agent-provider-editor-title">
              {form.id ? `编辑 ${form.name}` : `添加 ${selectedPreset.label}`}
            </strong>
            <small>凭据保存后只由 Rust 与系统凭据库读取。</small>
          </span>
        </header>

        <div className="agent-provider-editor-fields">
          <label className="settings-field">
            <span>渠道名称</span>
            <span className="settings-input-shell settings-input-shell--text">
              <input
                value={form.name}
                maxLength={textInputLimits.providerName}
                disabled={busy}
                placeholder="例如：工作用 OpenAI"
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    name: limitText(
                      event.target.value,
                      textInputLimits.providerName,
                    ),
                  }))
                }
              />
            </span>
          </label>

          <div className="settings-field">
            <div className="agent-provider-field-heading">
              <label htmlFor="agent-provider-protocol">API 协议</label>
              <small className="agent-provider-route-note" data-tone={
                explicitIncompatible ? "danger" : route?.maturity === "beta" ? "warning" : undefined
              }>
                {explicitIncompatible
                  ? `当前模型不支持 ${route?.label || "所选协议"}，请切换协议或模型。`
                  : route?.limitation
                    || (route?.maturity === "beta" ? "该协议仍处于 Beta，请先测试连接。" : "协议由 Rust 按当前模型解析。")}
              </small>
            </div>
            <ThemedSelect
              id="agent-provider-protocol"
              label="API 协议"
              value={form.protocolId}
              options={options}
              disabled={busy || !options.length}
              onValueChange={(protocolId) => {
                const resolved = protocolId === "auto"
                  ? recommendedProtocolId(selectedPreset, form)
                  : protocolId;
                const protocol = selectedPreset.protocols?.find(
                  (candidate) => candidate.id === resolved,
                );
                setForm((current) => ({
                  ...current,
                  protocolId,
                  baseUrl: current.baseUrlCustomized
                    ? current.baseUrl
                    : protocol?.baseUrl || current.baseUrl,
                }));
              }}
            />
          </div>

          <label className="settings-field agent-provider-editor-wide">
            <span>BASE_URL</span>
            <span className="settings-input-shell settings-input-shell--text">
              <input
                value={form.baseUrl}
                maxLength={textInputLimits.providerBaseUrl}
                disabled={busy}
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck="false"
                placeholder="https://api.example.com/v1"
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    baseUrl: limitText(
                      event.target.value,
                      textInputLimits.providerBaseUrl,
                    ),
                    baseUrlCustomized: true,
                  }))
                }
              />
            </span>
          </label>

          <label className="settings-field agent-provider-editor-wide">
            <span>API_KEY</span>
            <span className="settings-input-shell settings-input-shell--text">
              <input
                type="password"
                maxLength={textInputLimits.providerApiKey}
                value={
                  form.apiKey
                  || (!form.useEnvironmentKey
                    && form.hasStoredApiKey
                    && !editingStoredApiKey
                    ? storedApiKeyMask
                    : "")
                }
                disabled={busy || form.useEnvironmentKey}
                autoCapitalize="none"
                autoComplete="off"
                spellCheck="false"
                placeholder={
                  form.useEnvironmentKey
                    ? `使用 ${form.environmentVariable}`
                    : "输入渠道 API Key"
                }
                onFocus={() => {
                  if (form.hasStoredApiKey && !form.apiKey) {
                    setEditingStoredApiKey(true);
                  }
                }}
                onBlur={() => {
                  if (form.hasStoredApiKey && !form.apiKey) {
                    setEditingStoredApiKey(false);
                  }
                }}
                onChange={(event) => {
                  setEditingStoredApiKey(true);
                  setForm((current) => ({
                    ...current,
                    apiKey: limitText(
                      event.target.value,
                      textInputLimits.providerApiKey,
                    ),
                  }));
                }}
              />
            </span>
          </label>

          <div className="agent-environment-option agent-provider-editor-wide">
            <input
              id="agent-provider-use-environment-key"
              type="checkbox"
              checked={form.useEnvironmentKey}
              disabled={busy}
              onChange={(event) => {
                if (event.target.checked) setEditingStoredApiKey(false);
                setForm((current) => ({
                  ...current,
                  useEnvironmentKey: event.target.checked,
                  apiKey: event.target.checked ? "" : current.apiKey,
                }));
              }}
            />
            <label htmlFor="agent-provider-use-environment-key">
              从系统环境变量获取
            </label>
            <button
              type="button"
              className="settings-help__button"
              aria-label="查看各渠道 API Key 环境变量名称"
              onClick={() => setHelpOpen(true)}
            >
              <Question size={13} weight="bold" />
            </button>
            {form.useEnvironmentKey ? (
              <small data-available={form.hasEnvironmentApiKey || undefined}>
                {form.environmentVariable}
              </small>
            ) : null}
          </div>

          <label
            className="settings-field agent-provider-editor-wide"
            htmlFor="agent-provider-model-name"
          >
            <span>首选模型</span>
            <span className="settings-input-shell settings-input-shell--text">
              <input
                id="agent-provider-model-name"
                aria-label="首选模型"
                value={form.modelName}
                maxLength={textInputLimits.providerModelName}
                disabled={busy}
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck="false"
                placeholder="测试连接后可自动选择"
                onChange={(event) => setForm((current) => {
                  const next = {
                    ...current,
                    modelName: limitText(
                      event.target.value,
                      textInputLimits.providerModelName,
                    ),
                  };
                  if (!current.baseUrlCustomized && current.protocolId === "auto") {
                    const nextRouteId = recommendedProtocolId(selectedPreset, next);
                    const nextRoute = selectedPreset.protocols?.find(
                      (candidate) => candidate.id === nextRouteId,
                    );
                    next.baseUrl = nextRoute?.baseUrl || current.baseUrl;
                  }
                  return next;
                })}
              />
            </span>
            {form.models.length ? (
              <span className="agent-provider-model-suggestions">
                {form.models.slice(0, 8).map((model) => (
                  <button
                    key={model}
                    type="button"
                    data-selected={model === form.modelName || undefined}
                    onClick={() => setForm((current) => {
                      const next = { ...current, modelName: model };
                      if (!current.baseUrlCustomized && current.protocolId === "auto") {
                        const nextRouteId = recommendedProtocolId(selectedPreset, next);
                        const nextRoute = selectedPreset.protocols?.find(
                          (candidate) => candidate.id === nextRouteId,
                        );
                        next.baseUrl = nextRoute?.baseUrl || current.baseUrl;
                      }
                      return next;
                    })}
                  >
                    {model}
                  </button>
                ))}
              </span>
            ) : (
              <small>测试连接会检索可用模型；也可以先手动填写。</small>
            )}
          </label>

          {selectedPreset.id === "custom" ? (
            <div className="settings-field agent-provider-editor-wide">
              <label htmlFor="agent-provider-context-window">上下文窗口</label>
              <ThemedSelect
                id="agent-provider-context-window"
                label="上下文窗口"
                value={String(form.manualContextWindowTokens || 128000)}
                options={(registry.contextWindowOptions || [128000, 200000, 500000, 1000000, 2000000])
                  .map((tokens) => ({
                    value: String(tokens),
                    label: contextWindowLabels.get(tokens) || `${tokens} tokens`,
                  }))}
                disabled={busy}
                onValueChange={(value) => setForm((current) => ({
                  ...current,
                  manualContextWindowTokens: Number(value),
                }))}
              />
              <small>用于 API 未返回窗口大小时的高置信度回退；测试返回值会优先覆盖。</small>
            </div>
          ) : null}

          {editingProvider ? (
            <section
              className="agent-provider-test-result agent-provider-editor-wide"
              aria-label="连接与能力"
            >
              <header>
                <strong>连接与能力</strong>
                <small>{routeCapabilityCopy(editingProvider)} · 仅在手动测试后更新</small>
              </header>
              <dl>
                <div>
                  <dt>当前协议</dt>
                  <dd>{editingProvider.protocolLabel}</dd>
                </div>
                <div>
                  <dt>连接状态</dt>
                  <dd data-status={editingProvider.status}>
                    {statusCopy(editingProvider)}
                  </dd>
                </div>
                <div>
                  <dt>结构化输出</dt>
                  <dd data-capability={editingProvider.structuredOutputStatus || "unknown"}>
                    {capabilitySupportCopy(editingProvider.structuredOutputStatus)}
                  </dd>
                </div>
                <div>
                  <dt>工具调用</dt>
                  <dd data-capability={editingProvider.toolCallingStatus || "unknown"}>
                    {capabilitySupportCopy(editingProvider.toolCallingStatus)}
                  </dd>
                </div>
                <div>
                  <dt>多轮工具续接</dt>
                  <dd data-capability={editingProvider.multiTurnToolCallingStatus || "unknown"}>
                    {capabilitySupportCopy(editingProvider.multiTurnToolCallingStatus)}
                  </dd>
                </div>
              </dl>
            </section>
          ) : null}
        </div>

        <footer className="agent-provider-flow__actions">
          <span
            className="agent-config-feedback"
            data-tone={flowFeedback?.tone}
            role={flowFeedback?.tone === "danger" ? "alert" : "status"}
          >
            {flowFeedback?.text || " "}
          </span>
          <button
            type="button"
            className="secondary-button"
            disabled={busy || explicitIncompatible || !canSaveProvider(form)}
            onClick={() => void saveProvider({ testKind: "connection" })}
          >
            {actionState === "saving-for-connection" || actionState === "testing-connection" ? (
              <SpinnerGap size={15} className="spin" />
            ) : (
              <Pulse size={15} />
            )}
            测试连接
          </button>
          <button
            type="button"
            className="secondary-button"
            disabled={
              busy
              || explicitIncompatible
              || !canSaveProvider(form)
              || typeof client?.testAiProviderCapabilities !== "function"
            }
            onClick={() => void saveProvider({ testKind: "capabilities" })}
          >
            {actionState === "saving-for-capabilities"
              || actionState === "testing-capabilities" ? (
                <SpinnerGap size={15} className="spin" />
              ) : (
                <Pulse size={15} />
              )}
            测试能力
          </button>
          <button
            type="button"
            className="send-button"
            disabled={busy || explicitIncompatible || !canSaveProvider(form)}
            onClick={() => void saveProvider()}
          >
            {actionState === "saving" ? <SpinnerGap size={15} className="spin" /> : null}
            保存渠道
          </button>
        </footer>
      </section>
    );
  };

  const inFlow = view !== "list";
  return (
    <section className="settings-page agent-settings" aria-labelledby="settings-agent-title">
      <header className="settings-page__heading">
        <span>
          <p className="eyebrow">AGENT</p>
          <h3 id="settings-agent-title">Agent 配置</h3>
          <p>管理写信助理、正文优化与邮件翻译使用的模型渠道。</p>
        </span>
      </header>

      <section className="agent-config-card">
        <button
          type="button"
          className="agent-config-card__heading"
          aria-expanded={expanded}
          aria-controls="agent-model-configuration"
          onClick={() => setExpanded((current) => !current)}
        >
          <span>
            <strong>模型配置</strong>
            <small>
              {loadState === "loading"
                ? "正在读取配置…"
                : loadState === "error"
                  ? "配置读取失败"
                  : defaultProvider
                    ? `${defaultProvider.name} · ${defaultProvider.modelName}`
                    : `${registry.providers.length} 个渠道 · 尚未设置默认模型`}
            </small>
          </span>
          <CaretDown size={17} aria-hidden="true" />
        </button>

        {expanded ? (
          <div id="agent-model-configuration" className="agent-config-card__content">
            {loadState === "loading" ? (
              <p className="agent-config-loading" role="status">
                <SpinnerGap size={16} className="spin" />
                正在读取模型配置…
              </p>
            ) : loadState === "error" ? (
              <div className="agent-config-failure" role="alert">
                <span>
                  <strong>模型配置暂时无法读取</strong>
                  <small>其他设置仍可继续使用。</small>
                </span>
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => {
                    setLoadState("loading");
                    loadRegistry()
                      .then((next) => {
                        setRegistry(next);
                        setLoadState("ready");
                      })
                      .catch(() => setLoadState("error"));
                  }}
                >
                  重新加载
                </button>
              </div>
            ) : view === "presets" ? (
              renderPresetPicker()
            ) : view === "edit" ? (
              renderProviderEditor()
            ) : (
              renderProviderList()
            )}
          </div>
        ) : null}
      </section>

      {!inFlow ? (
        <>
          <section
            className="settings-preference-card agent-preference-card"
            aria-label="AI 翻译语言设置"
          >
            <div className="settings-preference-row agent-translation-language">
              <span>
                <strong>AI 翻译语言</strong>
                <small
                  className="agent-preference-feedback"
                  data-tone={translationFeedback?.tone}
                  role={translationFeedback?.tone === "danger" ? "alert" : "status"}
                >
                  {translationFeedback?.text || "选择阅读邮件时 AI 默认翻译成的语言。"}
                </small>
              </span>
              <ThemedSelect
                id="agent-translation-language"
                label="AI 翻译语言"
                value={registry.translationLanguage}
                options={registry.translationLanguages}
                disabled={loadState !== "ready" || translationState === "saving"}
                onValueChange={(value) => void updateTranslationLanguage(value)}
              />
            </div>
          </section>

          <section
            className="settings-preference-card agent-preference-card"
            aria-label="AI 助理默认状态"
          >
            <label className="settings-preference-row settings-preference-row--toggle agent-assistant-default">
              <span>
                <strong>默认开启 AI 助理</strong>
                <small>打开写信界面时自动展开右侧助理。</small>
              </span>
              <input
                type="checkbox"
                checked={Boolean(defaultAiAssistantOpen)}
                onChange={(event) =>
                  onDefaultAiAssistantOpenChange(event.target.checked)
                }
              />
            </label>
          </section>

          {children}
        </>
      ) : null}

      <ConsequentialConfirmDialog
        open={Boolean(deleteTarget)}
        title="删除 AI 渠道？"
        description={
          deleteTarget?.isDefault
            ? `“${deleteTarget?.name}”正在作为默认模型。删除后默认项会清空，但其他渠道不受影响。`
            : `将删除“${deleteTarget?.name}”的配置、模型记录和系统凭据。`
        }
        icon={<Trash size={22} weight="duotone" />}
        tone="danger"
        confirmLabel="删除渠道"
        pendingLabel="正在删除渠道…"
        isPending={actionState.startsWith("deleting:")}
        errorMessage={deleteError}
        onCancel={() => {
          setDeleteTarget(null);
          setDeleteError(null);
        }}
        onConfirm={() => void confirmDelete()}
      />

      {helpOpen ? (
        <div className="confirm-layer" onPointerDown={helpFocus.onBackdropPointerDown}>
          <section
            ref={helpFocus.dialogRef}
            className="confirm-dialog agent-environment-dialog"
            role="dialog"
            tabIndex={-1}
            aria-modal="true"
            aria-labelledby="agent-environment-title"
            aria-describedby="agent-environment-description"
            onKeyDown={helpFocus.onDialogKeyDown}
          >
            <header>
              <span className="confirm-dialog__icon" aria-hidden="true">
                <Question size={22} weight="duotone" />
              </span>
              <IconButton label="关闭环境变量说明" onClick={() => setHelpOpen(false)}>
                <X size={18} />
              </IconButton>
            </header>
            <h2 id="agent-environment-title">API Key 环境变量</h2>
            <p id="agent-environment-description">
              设置变量后重启 Mine Mail。启用环境变量时，表单中的密钥不会被使用。
            </p>
            <dl className="agent-environment-list vertical-scroll-surface">
              {registry.presets.map((preset) => (
                <div key={preset.id}>
                  <dt>{preset.label}</dt>
                  <dd><code>{preset.environmentVariable}</code></dd>
                </div>
              ))}
            </dl>
            <footer>
              <button
                ref={helpCloseRef}
                type="button"
                className="send-button"
                onClick={() => setHelpOpen(false)}
              >
                知道了
              </button>
            </footer>
          </section>
        </div>
      ) : null}
    </section>
  );
}

class AgentSettingsErrorBoundary extends Component {
  constructor(props) {
    super(props);
    this.state = { failed: false };
  }

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <section
        className="settings-page agent-settings"
        aria-labelledby="settings-agent-error-title"
      >
        <header className="settings-page__heading">
          <span>
            <p className="eyebrow">AGENT</p>
            <h3 id="settings-agent-error-title">Agent 配置</h3>
            <p>管理写信助理与邮件翻译使用的模型服务。</p>
          </span>
        </header>
        <section className="agent-config-card agent-config-failure" role="alert">
          <span>
            <strong>Agent 配置暂时无法显示</strong>
            <small>请重新加载配置，其他设置仍可继续使用。</small>
          </span>
          <button
            type="button"
            className="secondary-button"
            onClick={() => this.setState({ failed: false })}
          >
            重新加载
          </button>
        </section>
      </section>
    );
  }
}

export function AgentSettings({
  client = mailApi,
  defaultAiAssistantOpen = true,
  onDefaultAiAssistantOpenChange = () => {},
  children = null,
}) {
  return (
    <AgentSettingsErrorBoundary>
      <AgentSettingsContent
        client={client}
        defaultAiAssistantOpen={defaultAiAssistantOpen}
        onDefaultAiAssistantOpenChange={onDefaultAiAssistantOpenChange}
      >
        {children}
      </AgentSettingsContent>
    </AgentSettingsErrorBoundary>
  );
}
