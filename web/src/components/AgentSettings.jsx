import { Component, useEffect, useMemo, useRef, useState } from "react";
import {
  CaretDown,
  CheckCircle,
  MagnifyingGlass,
  Question,
  SpinnerGap,
  X,
} from "@phosphor-icons/react";
import { mailApi } from "../services/mailApi.js";
import { useBoundedDropdown } from "../hooks/useBoundedDropdown.js";
import { userFacingErrorMessage } from "../utils/userFacingError.js";
import { useConfirmDialogFocus } from "./ConfirmDialogPrimitives.jsx";
import { IconButton } from "./IconButton.jsx";
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

const initialConfiguration = Object.freeze({
  providerId: "custom",
  protocolId: "auto",
  resolvedProtocolId: "openai_chat_completions",
  baseUrl: "",
  modelName: "",
  apiKey: "",
  useEnvironmentKey: false,
  hasStoredApiKey: false,
  hasEnvironmentApiKey: false,
  environmentVariable: "AI_API_KEY",
  presets: [],
  translationLanguage: "zh-Hans",
  translationLanguages: fallbackTranslationLanguages,
});

const automaticSaveDelayMs = 600;
const storedApiKeyMask = "••••••••••••";

function normalizeConfiguration(config) {
  const value = config && typeof config === "object" ? config : {};
  return {
    ...initialConfiguration,
    ...value,
    apiKey: "",
    presets: Array.isArray(value.presets) ? value.presets : [],
    translationLanguages: Array.isArray(value.translationLanguages)
      && value.translationLanguages.length
      ? value.translationLanguages
      : fallbackTranslationLanguages,
  };
}

function configurationRequest(form) {
  return {
    providerId: form.providerId,
    protocolId: form.protocolId,
    baseUrl: form.baseUrl.trim(),
    modelName: form.modelName.trim(),
    useEnvironmentKey: form.useEnvironmentKey,
    translationLanguage: form.translationLanguage,
    apiKey: form.useEnvironmentKey ? "" : form.apiKey,
  };
}

function canSaveConfiguration(form) {
  if (
    !form.providerId?.trim()
    || !form.baseUrl?.trim()
    || !form.modelName?.trim()
    || !form.translationLanguage?.trim()
  ) {
    return false;
  }

  try {
    const url = new URL(form.baseUrl.trim());
    if (url.protocol !== "https:" && url.protocol !== "http:") return false;
  } catch {
    return false;
  }

  return Boolean(
    form.useEnvironmentKey
    || form.hasStoredApiKey
    || form.apiKey?.trim(),
  );
}

function providerFields(form) {
  return {
    protocolId: form.protocolId,
    resolvedProtocolId: form.resolvedProtocolId,
    baseUrl: form.baseUrl,
    modelName: form.modelName,
    useEnvironmentKey: form.useEnvironmentKey,
    hasStoredApiKey: form.hasStoredApiKey,
    hasEnvironmentApiKey: form.hasEnvironmentApiKey,
  };
}

function resolvedProtocolId(preset, protocolId) {
  if (!preset) return "openai_chat_completions";
  return protocolId === "auto"
    ? preset.recommendedProtocolId
    : protocolId;
}

function providerDraftKey(providerId, protocolId, preset) {
  return `${providerId}:${resolvedProtocolId(preset, protocolId)}`;
}

function protocolOptions(preset) {
  if (!preset) return [];
  const recommended = preset.protocols?.find(
    (protocol) => protocol.id === preset.recommendedProtocolId,
  );
  return [
    {
      value: "auto",
      label: `自动（推荐：${recommended?.label || "供应商默认"}）`,
    },
    ...(preset.protocols || []).map((protocol) => ({
      value: protocol.id,
      label: protocol.recommended
        ? `${protocol.label}（推荐）`
        : protocol.label,
    })),
  ];
}

function providerForm(current, preset, remembered = null) {
  const protocolId = preset.protocolId ?? remembered?.protocolId ?? "auto";
  const resolved = resolvedProtocolId(preset, protocolId);
  const protocol = preset.protocols?.find((candidate) => candidate.id === resolved);
  const configuration = remembered
    || preset.configurations?.find((candidate) => candidate.protocolId === resolved)
    || preset.configuration;
  return {
    ...current,
    providerId: preset.id,
    protocolId,
    resolvedProtocolId: resolved,
    baseUrl: configuration?.baseUrl ?? protocol?.baseUrl ?? preset.baseUrl,
    modelName:
      configuration?.modelName
      ?? protocol?.models?.[0]
      ?? preset.models?.[0]
      ?? "",
    apiKey: "",
    useEnvironmentKey: configuration?.useEnvironmentKey ?? false,
    environmentVariable: preset.environmentVariable,
    hasStoredApiKey: configuration?.hasStoredApiKey ?? false,
    hasEnvironmentApiKey: configuration?.hasEnvironmentApiKey ?? false,
  };
}

function statusMessage(error, fallback) {
  return userFacingErrorMessage(error, fallback);
}

function AgentSettingsContent({
  client,
  defaultAiAssistantOpen,
  onDefaultAiAssistantOpenChange,
  children,
}) {
  const [expanded, setExpanded] = useState(false);
  const [loadState, setLoadState] = useState("loading");
  const [form, setForm] = useState(initialConfiguration);
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  const [actionState, setActionState] = useState("idle");
  const [feedback, setFeedback] = useState(null);
  const [translationState, setTranslationState] = useState("idle");
  const [translationFeedback, setTranslationFeedback] = useState(null);
  const [editRevision, setEditRevision] = useState(0);
  const [helpOpen, setHelpOpen] = useState(false);
  const [editingStoredApiKey, setEditingStoredApiKey] = useState(false);
  const editRevisionRef = useRef(0);
  const savedRevisionRef = useRef(0);
  const blockedAutomaticRevisionRef = useRef(null);
  const providerDraftsRef = useRef({});
  const helpCloseRef = useRef(null);
  const modelMenuRef = useRef(null);
  const modelMenuLayout = useBoundedDropdown({
    open: modelMenuOpen,
    anchorRef: modelMenuRef,
    preferredMaxHeight: 166,
  });
  const helpFocus = useConfirmDialogFocus({
    open: helpOpen,
    initialFocusRef: helpCloseRef,
    onCancel: () => setHelpOpen(false),
  });

  useEffect(() => {
    let cancelled = false;
    setLoadState("loading");
    Promise.resolve()
      .then(() => {
        if (typeof client?.getAiConfig !== "function") {
          throw new Error("AI configuration client is unavailable");
        }
        return client.getAiConfig();
      })
      .then((config) => {
        if (cancelled) return;
        setForm(normalizeConfiguration(config));
        setEditingStoredApiKey(false);
        editRevisionRef.current = 0;
        savedRevisionRef.current = 0;
        blockedAutomaticRevisionRef.current = null;
        providerDraftsRef.current = {};
        setEditRevision(0);
        setLoadState("ready");
      })
      .catch((error) => {
        if (cancelled) return;
        setFeedback({
          tone: "danger",
          text: statusMessage(error, "AI 配置读取失败，请重试。"),
        });
        setLoadState("error");
      });
    return () => {
      cancelled = true;
    };
  }, [client]);

  useEffect(() => {
    if (!modelMenuOpen) return undefined;
    const closeOnOutsidePointer = (event) => {
      if (!modelMenuRef.current?.contains(event.target)) {
        setModelMenuOpen(false);
      }
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [modelMenuOpen]);

  const selectedPreset = useMemo(
    () => form.presets.find((preset) => preset.id === form.providerId),
    [form.presets, form.providerId],
  );
  const selectedProtocol = useMemo(
    () => selectedPreset?.protocols?.find(
      (protocol) => protocol.id === form.resolvedProtocolId,
    ),
    [form.resolvedProtocolId, selectedPreset],
  );
  const models = selectedProtocol?.models || selectedPreset?.models || [];
  const availableProtocolOptions = useMemo(
    () => protocolOptions(selectedPreset),
    [selectedPreset],
  );
  const busy = ["saving", "switching", "models", "testing"].includes(actionState);

  const resetModelResults = () => {
    setModelMenuOpen(false);
  };

  const markConfigurationEdited = () => {
    editRevisionRef.current += 1;
    blockedAutomaticRevisionRef.current = null;
    setEditRevision(editRevisionRef.current);
  };

  const updateField = (field, value, { resetModels = false } = {}) => {
    setForm((current) => ({ ...current, [field]: value }));
    markConfigurationEdited();
    setFeedback(null);
    if (resetModels) resetModelResults();
  };

  const chooseProvider = async (preset) => {
    if (busy || preset.id === form.providerId) return;

    setActionState("switching");
    setFeedback(null);
    resetModelResults();
    let phase = "current";

    try {
      let source = form;
      if (
        editRevisionRef.current > savedRevisionRef.current
        && canSaveConfiguration(form)
      ) {
        source = normalizeConfiguration(
          await client.saveAiConfig(configurationRequest(form)),
        );
        savedRevisionRef.current = editRevisionRef.current;
        delete providerDraftsRef.current[
          providerDraftKey(form.providerId, form.protocolId, selectedPreset)
        ];
      } else if (editRevisionRef.current > savedRevisionRef.current) {
        providerDraftsRef.current[
          providerDraftKey(form.providerId, form.protocolId, selectedPreset)
        ] = providerFields(form);
      }

      const currentPreset = source.presets.find(
        (candidate) => candidate.id === preset.id,
      ) || preset;
      const next = providerForm(
        source,
        currentPreset,
        providerDraftsRef.current[
          providerDraftKey(
            preset.id,
            currentPreset.protocolId ?? "auto",
            currentPreset,
          )
        ],
      );
      setEditingStoredApiKey(false);
      setForm(next);
      editRevisionRef.current += 1;
      savedRevisionRef.current = editRevisionRef.current;
      blockedAutomaticRevisionRef.current = null;
      setEditRevision(editRevisionRef.current);

      if (currentPreset.configuration && canSaveConfiguration(next)) {
        phase = "target";
        const activated = normalizeConfiguration(
          await client.saveAiConfig(configurationRequest(next)),
        );
        delete providerDraftsRef.current[
          providerDraftKey(preset.id, next.protocolId, currentPreset)
        ];
        setForm(activated);
        setFeedback({
          tone: "success",
          text: `已切换至 ${currentPreset.label}。`,
        });
      }
    } catch (error) {
      setFeedback({
        tone: "danger",
        text: statusMessage(
          error,
          phase === "current"
            ? "当前渠道配置保存失败，请重试后再切换。"
            : "渠道切换失败，请检查配置后重试。",
        ),
      });
    } finally {
      setActionState("idle");
    }
  };

  const chooseProtocol = (protocolId) => {
    if (busy || protocolId === form.protocolId || !selectedPreset) return;
    const currentKey = providerDraftKey(
      form.providerId,
      form.protocolId,
      selectedPreset,
    );
    if (editRevisionRef.current > savedRevisionRef.current) {
      providerDraftsRef.current[currentKey] = providerFields(form);
    }
    const targetKey = providerDraftKey(
      form.providerId,
      protocolId,
      selectedPreset,
    );
    const next = providerForm(
      form,
      { ...selectedPreset, protocolId },
      providerDraftsRef.current[targetKey],
    );
    setForm(next);
    setEditingStoredApiKey(false);
    resetModelResults();
    markConfigurationEdited();
    setFeedback(null);
  };

  const retrieveModels = async () => {
    setActionState("models");
    setFeedback(null);
    setModelMenuOpen(false);
    try {
      const availableModels = await client.listAiModels(configurationRequest(form));
      setForm((current) => ({
        ...current,
        modelName: current.modelName || availableModels[0] || "",
        presets: current.presets.map((preset) =>
          preset.id === current.providerId
            ? {
                ...preset,
                models: availableModels,
                protocols: (preset.protocols || []).map((protocol) =>
                  protocol.id === current.resolvedProtocolId
                    ? { ...protocol, models: availableModels }
                    : protocol,
                ),
              }
            : preset,
        ),
      }));
      markConfigurationEdited();
      setModelMenuOpen(true);
      setFeedback({
        tone: "success",
        text: `已检索到 ${availableModels.length} 个可用模型。`,
      });
    } catch (error) {
      setModelMenuOpen(false);
      setFeedback({
        tone: "danger",
        text: statusMessage(error, "可用模型检索失败，请检查配置。"),
      });
    } finally {
      setActionState("idle");
    }
  };

  const testConnection = async () => {
    setActionState("testing");
    setFeedback(null);
    try {
      const result = await client.testAiConnection(configurationRequest(form));
      setFeedback({
        tone: "success",
        text: `连接成功 · ${result.latencyMs} ms`,
      });
    } catch (error) {
      setFeedback({
        tone: "danger",
        text: statusMessage(error, "连接测试失败，请检查配置。"),
      });
    } finally {
      setActionState("idle");
    }
  };

  const saveConfiguration = async ({ automatic = false, revision = editRevision } = {}) => {
    if (!canSaveConfiguration(form)) {
      if (!automatic) {
        setFeedback({
          tone: "danger",
          text: "请先完整填写模型地址、API Key 和模型名称。",
        });
      }
      return;
    }

    const request = configurationRequest(form);
    setActionState("saving");
    setFeedback(
      automatic
        ? { tone: "neutral", text: "正在自动保存配置…" }
        : null,
    );
    try {
      const saved = await client.saveAiConfig(request);
      const savedPreset = saved.presets?.find(
        (preset) => preset.id === (saved.providerId ?? form.providerId),
      );
      delete providerDraftsRef.current[
        providerDraftKey(
          saved.providerId ?? form.providerId,
          saved.protocolId ?? form.protocolId,
          savedPreset ?? selectedPreset,
        )
      ];
      savedRevisionRef.current = Math.max(savedRevisionRef.current, revision);
      blockedAutomaticRevisionRef.current = null;
      if (editRevisionRef.current === revision) {
        setForm(normalizeConfiguration(saved));
        setEditingStoredApiKey(false);
        setFeedback({
          tone: "success",
          text: automatic ? "配置已自动保存。" : "模型配置已保存。",
        });
      }
    } catch (error) {
      blockedAutomaticRevisionRef.current = revision;
      setFeedback({
        tone: "danger",
        text: statusMessage(
          error,
          automatic
            ? "配置自动保存失败，可修改后重试或点击保存配置。"
            : "模型配置保存失败，请检查后重试。",
        ),
      });
    } finally {
      setActionState("idle");
    }
  };

  const updateTranslationLanguage = async (languageId) => {
    if (languageId === form.translationLanguage || translationState === "saving") {
      return;
    }

    const previousLanguage = form.translationLanguage;
    setForm((current) => ({ ...current, translationLanguage: languageId }));
    setTranslationState("saving");
    setTranslationFeedback({ tone: "neutral", text: "正在保存翻译语言…" });
    try {
      if (typeof client?.setAiTranslationLanguage !== "function") {
        throw new Error("AI translation language client is unavailable");
      }
      const saved = normalizeConfiguration(
        await client.setAiTranslationLanguage(languageId),
      );
      setForm((current) => ({
        ...current,
        translationLanguage: saved.translationLanguage,
        translationLanguages: saved.translationLanguages,
      }));
      setTranslationFeedback({ tone: "success", text: "翻译语言已保存。" });
    } catch (error) {
      setForm((current) => ({
        ...current,
        translationLanguage:
          current.translationLanguage === languageId
            ? previousLanguage
            : current.translationLanguage,
      }));
      setTranslationFeedback({
        tone: "danger",
        text: statusMessage(error, "翻译语言保存失败，请重试。"),
      });
    } finally {
      setTranslationState("idle");
    }
  };

  useEffect(() => {
    if (
      loadState !== "ready"
      || actionState !== "idle"
      || editRevision <= savedRevisionRef.current
      || blockedAutomaticRevisionRef.current === editRevision
      || !canSaveConfiguration(form)
    ) {
      return undefined;
    }

    const timer = window.setTimeout(() => {
      void saveConfiguration({ automatic: true, revision: editRevision });
    }, automaticSaveDelayMs);
    return () => window.clearTimeout(timer);
  }, [actionState, editRevision, form, loadState]);

  return (
    <section className="settings-page agent-settings" aria-labelledby="settings-agent-title">
      <header className="settings-page__heading">
        <span>
          <p className="eyebrow">AGENT</p>
          <h3 id="settings-agent-title">Agent 配置</h3>
          <p>配置写信助理与邮件翻译使用的模型服务。API Key 由系统凭据库保管。</p>
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
                  : selectedPreset?.label || "选择模型供应商"}
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
            ) : (
              <>
                <fieldset className="agent-provider-presets" disabled={busy}>
                  <legend>预设供应商</legend>
                  <div>
                    {form.presets.map((preset) => (
                      <button
                        key={preset.id}
                        type="button"
                        data-selected={preset.id === form.providerId}
                        onClick={() => chooseProvider(preset)}
                      >
                        {preset.label}
                      </button>
                    ))}
                  </div>
                </fieldset>

                <div className="agent-config-fields">
                  <div className="settings-field agent-protocol-field">
                    <label htmlFor="agent-api-protocol">API 协议</label>
                    <ThemedSelect
                      id="agent-api-protocol"
                      label="API 协议"
                      value={form.protocolId}
                      options={availableProtocolOptions}
                      disabled={busy || availableProtocolOptions.length === 0}
                      onValueChange={chooseProtocol}
                    />
                    <small>
                      自动会跟随当前供应商的推荐协议；切换协议后会分别保留地址、模型和检索结果。
                    </small>
                  </div>

                  <label className="settings-field">
                    <span>BASE_URL</span>
                    <span className="settings-input-shell settings-input-shell--text">
                      <input
                        value={form.baseUrl}
                        disabled={busy}
                        autoCapitalize="none"
                        autoCorrect="off"
                        spellCheck="false"
                        placeholder="https://api.example.com/v1"
                        onChange={(event) =>
                          updateField("baseUrl", event.target.value, {
                            resetModels: true,
                          })
                        }
                      />
                    </span>
                  </label>

                  <label className="settings-field">
                    <span>API_KEY</span>
                    <span className="settings-input-shell settings-input-shell--text">
                      <input
                        type="password"
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
                            : "输入供应商 API Key"
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
                          updateField("apiKey", event.target.value, {
                            resetModels: true,
                          });
                        }}
                      />
                    </span>
                  </label>

                  <div className="agent-environment-option">
                    <input
                      id="agent-use-environment-key"
                      type="checkbox"
                      checked={form.useEnvironmentKey}
                      disabled={busy}
                      onChange={(event) => {
                        if (event.target.checked) {
                          setEditingStoredApiKey(false);
                        }
                        setForm((current) => ({
                          ...current,
                          useEnvironmentKey: event.target.checked,
                          apiKey: event.target.checked ? "" : current.apiKey,
                        }));
                        markConfigurationEdited();
                        setFeedback(null);
                        resetModelResults();
                      }}
                    />
                    <label htmlFor="agent-use-environment-key">从系统环境变量获取</label>
                    <button
                      type="button"
                      className="settings-help__button"
                      aria-label="查看各供应商 API Key 环境变量名称"
                      aria-haspopup="dialog"
                      aria-expanded={helpOpen}
                      onClick={(event) => {
                        event.preventDefault();
                        setHelpOpen(true);
                      }}
                    >
                      <Question size={13} weight="bold" />
                    </button>
                    {form.useEnvironmentKey ? (
                      <small data-available={form.hasEnvironmentApiKey || undefined}>
                        {form.hasEnvironmentApiKey ? "获取成功" : "保存后需重启应用以读取"}
                      </small>
                    ) : null}
                  </div>

                  <div className="settings-field">
                    <label htmlFor="agent-model-name">MODEL_NAME</label>
                    <span className="agent-model-control" ref={modelMenuRef}>
                      <span className="settings-input-shell settings-input-shell--text">
                        <input
                          id="agent-model-name"
                          role="combobox"
                          aria-autocomplete="list"
                          aria-expanded={modelMenuOpen}
                          aria-controls={models.length ? "agent-model-options" : undefined}
                          value={form.modelName}
                          disabled={busy}
                          autoCapitalize="none"
                          autoCorrect="off"
                          spellCheck="false"
                          placeholder="输入或检索模型名称"
                          onChange={(event) =>
                            updateField("modelName", event.target.value)
                          }
                        />
                      </span>
                      <IconButton
                        className="agent-model-control__toggle"
                        label="展开可用模型"
                        disabled={busy || models.length === 0}
                        onClick={() => setModelMenuOpen((current) => !current)}
                      >
                        <CaretDown size={15} />
                      </IconButton>
                      <button
                        type="button"
                        className="agent-model-control__search"
                        disabled={busy || !form.baseUrl.trim()}
                        onClick={() => void retrieveModels()}
                      >
                        {actionState === "models" ? (
                          <SpinnerGap size={15} className="spin" />
                        ) : (
                          <MagnifyingGlass size={15} />
                        )}
                        检索可用模型
                      </button>
                      {modelMenuOpen && models.length ? (
                        <div
                          id="agent-model-options"
                          className="agent-model-options vertical-scroll-surface"
                          role="listbox"
                          aria-label="可用模型"
                          style={{ maxHeight: `${modelMenuLayout.maxHeight}px` }}
                        >
                          {models.map((model) => (
                            <button
                              key={model}
                              type="button"
                              role="option"
                              aria-selected={model === form.modelName}
                              onClick={() => {
                                updateField("modelName", model);
                                setModelMenuOpen(false);
                              }}
                            >
                              <span>{model}</span>
                              {model === form.modelName ? <CheckCircle size={15} weight="fill" /> : null}
                            </button>
                          ))}
                        </div>
                      ) : null}
                    </span>
                    <small>可直接选择预设模型；检索成功后会更新并保存当前供应商的列表。</small>
                  </div>

                </div>

                <div className="agent-config-actions">
                  <span
                    className="agent-config-feedback"
                    data-tone={feedback?.tone}
                    role={feedback?.tone === "danger" ? "alert" : "status"}
                    aria-live="polite"
                  >
                    {feedback?.text || " "}
                  </span>
                  <button
                    type="button"
                    className="secondary-button"
                    disabled={busy || !form.modelName.trim()}
                    onClick={() => void testConnection()}
                  >
                    {actionState === "testing" ? <SpinnerGap size={15} className="spin" /> : null}
                    测试连接
                  </button>
                  <button
                    type="button"
                    className="send-button"
                    disabled={
                      busy
                      || loadState !== "ready"
                      || !canSaveConfiguration(form)
                    }
                    onClick={() => void saveConfiguration()}
                  >
                    {actionState === "saving" ? <SpinnerGap size={15} className="spin" /> : null}
                    保存配置
                  </button>
                </div>
              </>
            )}
          </div>
        ) : null}
      </section>

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
              aria-live="polite"
            >
              {translationFeedback?.text || "选择阅读邮件时 AI 默认翻译成的语言。"}
            </small>
          </span>
          <ThemedSelect
            id="agent-translation-language"
            label="AI 翻译语言"
            value={form.translationLanguage}
            options={form.translationLanguages}
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
              在系统中设置对应变量后重启 Mine Mail。启用此选项时，输入框中的密钥不会被使用。
            </p>
            <dl className="agent-environment-list vertical-scroll-surface">
              {form.presets.map((preset) => (
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
            <p>配置写信助理与邮件翻译使用的模型服务。</p>
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
