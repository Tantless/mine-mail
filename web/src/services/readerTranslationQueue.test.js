import { describe, expect, it, vi } from "vitest";
import {
  createReaderTranslationQueue,
  translationSourceFingerprint,
} from "./readerTranslationQueue.js";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

async function flushPromises() {
  await Promise.resolve();
  await Promise.resolve();
}

describe("reader translation queue", () => {
  it("fingerprints the complete source without exposing it in the key", () => {
    const first = translationSourceFingerprint([
      { id: "message-subject", format: "plain", content: "private subject" },
      { id: "body", format: "plain", content: "private message" },
    ]);
    const same = translationSourceFingerprint([
      { id: "message-subject", format: "plain", content: "private subject" },
      { id: "body", format: "plain", content: "private message" },
    ]);
    const changed = translationSourceFingerprint([
      { id: "message-subject", format: "plain", content: "changed subject" },
      { id: "body", format: "plain", content: "private message" },
    ]);
    const changedBody = translationSourceFingerprint([
      { id: "message-subject", format: "plain", content: "private subject" },
      { id: "body", format: "plain", content: "changed message" },
    ]);

    expect(first).toBe(same);
    expect(first).not.toBe(changed);
    expect(first).not.toBe(changedBody);
    expect(first).not.toContain("private subject");
    expect(first).not.toContain("private message");
  });

  it("runs at most two messages concurrently and keeps completed results", async () => {
    const queue = createReaderTranslationQueue({ maxConcurrent: 2 });
    const jobs = [deferred(), deferred(), deferred()];
    const runs = jobs.map((job) => vi.fn(() => job.promise));

    for (let index = 0; index < jobs.length; index += 1) {
      queue.enqueue({
        key: `message:${index}`,
        sourceFingerprint: `source:${index}`,
        language: "ja",
        run: runs[index],
      });
    }
    await flushPromises();

    expect(runs[0]).toHaveBeenCalledOnce();
    expect(runs[1]).toHaveBeenCalledOnce();
    expect(runs[2]).not.toHaveBeenCalled();
    expect(queue.getSnapshot("message:2", "source:2").status).toBe("queued");

    jobs[0].resolve({ parts: [{ id: "body", content: "一" }] });
    await flushPromises();
    await flushPromises();

    expect(runs[2]).toHaveBeenCalledOnce();
    expect(queue.getSnapshot("message:0", "source:0")).toMatchObject({
      status: "completed",
      language: "ja",
      showTranslated: true,
      translatedParts: [{ id: "body", content: "一" }],
    });
    expect(queue.getSnapshot("message:0", "different-source")).toBeNull();

    jobs[1].resolve({ parts: [{ id: "body", content: "二" }] });
    jobs[2].resolve({ parts: [{ id: "body", content: "三" }] });
    await flushPromises();
  });

  it("retains the previous translation while replacing its language", async () => {
    const queue = createReaderTranslationQueue();
    const first = deferred();
    queue.enqueue({
      key: "message:1",
      sourceFingerprint: "source:1",
      language: "ja",
      run: () => first.promise,
    });
    first.resolve({ parts: [{ id: "body", content: "日本語" }] });
    await flushPromises();
    await flushPromises();

    const second = deferred();
    queue.enqueue({
      key: "message:1",
      sourceFingerprint: "source:1",
      language: "fr",
      run: () => second.promise,
    });
    await flushPromises();

    expect(queue.getSnapshot("message:1", "source:1")).toMatchObject({
      status: "loading",
      language: "ja",
      requestedLanguage: "fr",
      translatedParts: [{ id: "body", content: "日本語" }],
    });

    const failure = new Error("provider detail");
    failure.userMessage = "法语翻译失败，请重试。";
    second.reject(failure);
    await flushPromises();
    await flushPromises();

    expect(queue.getSnapshot("message:1", "source:1")).toMatchObject({
      status: "completed",
      language: "ja",
      translatedParts: [{ id: "body", content: "日本語" }],
      error: "法语翻译失败，请重试。",
    });
  });

  it("deduplicates an identical in-flight translation", async () => {
    const queue = createReaderTranslationQueue();
    const job = deferred();
    const run = vi.fn(() => job.promise);
    const request = {
      key: "message:1",
      sourceFingerprint: "source:1",
      language: "en",
      run,
    };

    expect(queue.enqueue(request)).toBe(true);
    expect(queue.enqueue(request)).toBe(false);
    await flushPromises();
    expect(run).toHaveBeenCalledOnce();

    job.resolve({ parts: [{ id: "body", content: "English" }] });
    await flushPromises();
  });
});
