import { invoke } from "@tauri-apps/api/core";
import { mockDrafts, mockMessages } from "../data/mockMail.js";

// Design QA can opt into the deterministic mock inbox inside the desktop shell.
// Production builds never enable this branch.
const isDesignPreview =
  import.meta.env.DEV && import.meta.env.VITE_MINE_MAIL_DEMO === "1";

export const isTauriRuntime =
  typeof window !== "undefined" &&
  "__TAURI_INTERNALS__" in window;

export const isTauri = !isDesignPreview && isTauriRuntime;

const wait = (milliseconds) =>
  new Promise((resolve) => window.setTimeout(resolve, milliseconds));

let webMessages = structuredClone(mockMessages);
let webDrafts = structuredClone(mockDrafts);

function webOnly(action) {
  return async (...args) => {
    await wait(260);
    return action(...args);
  };
}

export const mailApi = {
  async listInbox(limit = 50) {
    if (isTauri) return invoke("list_inbox");
    return webOnly(() => structuredClone(webMessages.slice(0, limit)))();
  },

  async syncInbox(limit = 100) {
    if (isTauri) return invoke("sync_inbox");
    return webOnly(() => ({
      mailbox: "INBOX",
      remote_total: webMessages.length,
      fetched: 2,
      updated_flags: 1,
      removed: 0,
      cached_total: webMessages.length,
      uid_validity_reset: false,
    }))();
  },

  async fetchMessage(uid) {
    if (isTauri) return invoke("fetch_message", { uid });
    return webOnly(() => structuredClone(webMessages.find((mail) => mail.uid === uid)))();
  },

  async listDrafts() {
    if (isTauri) return invoke("list_drafts");
    return webOnly(() => structuredClone(webDrafts))();
  },

  async saveDraft(request) {
    if (isTauri) return invoke("save_draft", { request });
    return webOnly(() => {
      const draft = {
        ...structuredClone(request),
        id: crypto.randomUUID(),
        status: "local",
        updated_at: new Date().toISOString(),
      };
      webDrafts = [draft, ...webDrafts];
      return draft;
    })();
  },

  async sendCompose(request) {
    if (isTauri) {
      const draft = await invoke("save_draft", { request });
      const confirmedRecipients = [
        ...request.to,
        ...request.cc,
        ...request.bcc,
      ];
      return invoke("send_draft", {
        draftId: draft.id,
        confirmedRecipients,
      });
    }
    return webOnly(() => ({
      id: crypto.randomUUID(),
      recipients: [...request.to, ...request.cc, ...request.bcc],
      status: "sent",
      sent_at: new Date().toISOString(),
    }))();
  },

  async checkConnections() {
    if (isTauri) return invoke("check_connections");
    return webOnly(() => ({ imap_ok: true, smtp_ok: true }))();
  },
};
