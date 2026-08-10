const DEFAULT_MAX_CONCURRENT = 2;
const DEFAULT_MAX_ENTRIES = 50;

function boundedPositiveInteger(value, fallback) {
  return Number.isInteger(value) && value > 0 ? value : fallback;
}

function updateFingerprint(hash, value) {
  const text = String(value ?? "");
  let next = hash;
  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index);
    next ^= code & 0xff;
    next = Math.imul(next, 0x01000193);
    next ^= code >>> 8;
    next = Math.imul(next, 0x01000193);
  }
  return next >>> 0;
}

export function translationSourceFingerprint(parts) {
  const sourceParts = Array.isArray(parts) ? parts : [];
  let hash = 0x811c9dc5;
  let sourceUnits = 0;
  for (const part of sourceParts) {
    const id = String(part?.id ?? "");
    const format = String(part?.format ?? "");
    const content = String(part?.content ?? "");
    sourceUnits += id.length + format.length + content.length;
    hash = updateFingerprint(hash, id);
    hash = updateFingerprint(hash, "\0");
    hash = updateFingerprint(hash, format);
    hash = updateFingerprint(hash, "\0");
    hash = updateFingerprint(hash, content);
    hash = updateFingerprint(hash, "\u0001");
  }
  return `${sourceParts.length}:${sourceUnits}:${hash.toString(16).padStart(8, "0")}`;
}

function taskErrorMessage(error) {
  if (
    error
    && typeof error === "object"
    && Object.prototype.hasOwnProperty.call(error, "userMessage")
  ) {
    return typeof error.userMessage === "string" ? error.userMessage : null;
  }
  return "AI 翻译失败，请重试。";
}

function recordQueueEvent(event, { activeCount, queuedCount, language, status }) {
  if (!import.meta.env.DEV) return;
  console.info("[reader-translation-queue]", {
    event,
    activeCount,
    queuedCount,
    language,
    status,
  });
}

export function createReaderTranslationQueue(options = {}) {
  const maxConcurrent = boundedPositiveInteger(
    options.maxConcurrent,
    DEFAULT_MAX_CONCURRENT,
  );
  const maxEntries = boundedPositiveInteger(options.maxEntries, DEFAULT_MAX_ENTRIES);
  const entries = new Map();
  const listeners = new Set();
  const waiting = [];
  let activeCount = 0;
  let sequence = 0;

  const publish = () => {
    for (const listener of listeners) listener();
  };

  const prune = () => {
    if (entries.size <= maxEntries) return;
    const removable = [...entries.entries()]
      .filter(([, entry]) => !["queued", "loading"].includes(entry.status))
      .sort((left, right) => left[1].updatedAt - right[1].updatedAt);
    for (const [key] of removable) {
      if (entries.size <= maxEntries) break;
      entries.delete(key);
    }
  };

  const getSnapshot = (key, sourceFingerprint) => {
    const entry = key ? entries.get(key) : null;
    return entry?.sourceFingerprint === sourceFingerprint ? entry : null;
  };

  const completeTask = (task, result) => {
    const current = entries.get(task.key);
    if (
      current?.jobId === task.jobId
      && current.sourceFingerprint === task.sourceFingerprint
    ) {
      entries.set(task.key, {
        ...current,
        status: "completed",
        language: task.language,
        requestedLanguage: null,
        translatedParts: Array.isArray(result?.parts) ? result.parts : [],
        showTranslated: true,
        error: null,
        notice: typeof result?.notice === "string" ? result.notice : null,
        updatedAt: Date.now(),
      });
      recordQueueEvent("completed", {
        activeCount: Math.max(0, activeCount - 1),
        queuedCount: waiting.length,
        language: task.language,
        status: "completed",
      });
    }
  };

  const failTask = (task, error) => {
    const current = entries.get(task.key);
    if (
      current?.jobId === task.jobId
      && current.sourceFingerprint === task.sourceFingerprint
    ) {
      const hasPreviousResult = Array.isArray(current.translatedParts);
      entries.set(task.key, {
        ...current,
        status: hasPreviousResult ? "completed" : "error",
        requestedLanguage: null,
        showTranslated: hasPreviousResult ? current.showTranslated : false,
        error: taskErrorMessage(error),
        updatedAt: Date.now(),
      });
      recordQueueEvent("failed", {
        activeCount: Math.max(0, activeCount - 1),
        queuedCount: waiting.length,
        language: task.language,
        status: hasPreviousResult ? "completed" : "error",
      });
    }
  };

  const pump = () => {
    while (activeCount < maxConcurrent && waiting.length) {
      const task = waiting.shift();
      const current = entries.get(task.key);
      if (
        current?.jobId !== task.jobId
        || current.sourceFingerprint !== task.sourceFingerprint
      ) {
        continue;
      }
      activeCount += 1;
      entries.set(task.key, {
        ...current,
        status: "loading",
        updatedAt: Date.now(),
      });
      recordQueueEvent("started", {
        activeCount,
        queuedCount: waiting.length,
        language: task.language,
        status: "loading",
      });
      publish();
      Promise.resolve()
        .then(task.run)
        .then((result) => completeTask(task, result))
        .catch((error) => failTask(task, error))
        .finally(() => {
          activeCount = Math.max(0, activeCount - 1);
          prune();
          publish();
          pump();
        });
    }
  };

  const enqueue = ({
    key,
    sourceFingerprint,
    language,
    run,
  }) => {
    const normalizedKey = String(key || "").trim();
    const normalizedLanguage = String(language || "").trim();
    if (!normalizedKey || !sourceFingerprint || !normalizedLanguage || typeof run !== "function") {
      return false;
    }
    const existing = entries.get(normalizedKey);
    const sameSource = existing?.sourceFingerprint === sourceFingerprint;
    if (
      sameSource
      && ["queued", "loading"].includes(existing.status)
      && existing.requestedLanguage === normalizedLanguage
    ) {
      return false;
    }
    if (
      sameSource
      && existing?.status === "completed"
      && existing.language === normalizedLanguage
      && Array.isArray(existing.translatedParts)
    ) {
      entries.set(normalizedKey, {
        ...existing,
        showTranslated: true,
        error: null,
        updatedAt: Date.now(),
      });
      publish();
      return false;
    }

    const jobId = sequence + 1;
    sequence = jobId;
    entries.set(normalizedKey, {
      jobId,
      sourceFingerprint,
      status: "queued",
      language: sameSource ? existing.language : null,
      requestedLanguage: normalizedLanguage,
      translatedParts: sameSource ? existing.translatedParts : null,
      showTranslated: sameSource ? existing.showTranslated : false,
      error: null,
      notice: sameSource ? existing.notice : null,
      updatedAt: Date.now(),
    });
    waiting.push({
      key: normalizedKey,
      sourceFingerprint,
      language: normalizedLanguage,
      run,
      jobId,
    });
    prune();
    recordQueueEvent("queued", {
      activeCount,
      queuedCount: waiting.length,
      language: normalizedLanguage,
      status: "queued",
    });
    publish();
    pump();
    return true;
  };

  const setShowTranslated = (key, sourceFingerprint, showTranslated) => {
    const current = getSnapshot(key, sourceFingerprint);
    if (!current || !Array.isArray(current.translatedParts)) return;
    entries.set(key, {
      ...current,
      showTranslated: Boolean(showTranslated),
      updatedAt: Date.now(),
    });
    publish();
  };

  return {
    enqueue,
    getSnapshot,
    setShowTranslated,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
