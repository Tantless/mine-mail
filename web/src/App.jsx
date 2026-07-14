import { useCallback, useEffect, useMemo, useState } from "react";
import { Minus, Square, X } from "@phosphor-icons/react";
import { emptyCompose } from "./data/mockMail.js";
import { mailApi, isTauriRuntime } from "./services/mailApi.js";
import { Sidebar } from "./components/Sidebar.jsx";
import { MailList } from "./components/MailList.jsx";
import { MessageView } from "./components/MessageView.jsx";
import { ComposePanel } from "./components/ComposePanel.jsx";
import { SendConfirmDialog } from "./components/SendConfirmDialog.jsx";
import { Toast } from "./components/Toast.jsx";
import { hasFlag } from "./utils/formatters.js";

const folderLabels = {
  inbox: "收件箱",
  starred: "已加星标",
  sent: "已发送",
  drafts: "草稿",
  archive: "归档",
  trash: "垃圾箱",
};

const validThemes = new Set(["daylight", "night", "dusk", "forest"]);

function getInitialTheme() {
  const saved = window.localStorage.getItem("mine-mail-theme");
  return validThemes.has(saved) ? saved : "daylight";
}

function addFlag(message, flag) {
  const flags = new Set(message.flags || []);
  flags.add(flag);
  return { ...message, flags: [...flags] };
}

function toggleFlag(message, flag) {
  const flags = new Set(message.flags || []);
  if (hasFlag(message, flag)) {
    for (const value of flags) {
      if (value.toLowerCase() === flag.toLowerCase()) flags.delete(value);
    }
  } else {
    flags.add(flag);
  }
  return { ...message, flags: [...flags] };
}

function toDraftMessage(draft, index) {
  return {
    id: draft.id,
    uid: `draft-${draft.id}`,
    kind: "draft",
    subject: draft.subject || "（无主题草稿）",
    sender: { name: "草稿", email: "" },
    to: (draft.to || []).map((email) => ({ name: null, email })),
    sent_at: draft.updated_at,
    flags: ["\\Seen"],
    preview: draft.body_text || "空白草稿",
    body_text: draft.body_text,
    attachment_names: [],
    body_fetched: true,
    draft,
    sortIndex: index,
  };
}

export function App() {
  const [theme, setTheme] = useState(getInitialTheme);
  const [activeFolder, setActiveFolder] = useState("inbox");
  const [messages, setMessages] = useState([]);
  const [drafts, setDrafts] = useState([]);
  const [sentMessages, setSentMessages] = useState([]);
  const [selectedUid, setSelectedUid] = useState(null);
  const [selectedMessage, setSelectedMessage] = useState(null);
  const [isMessageLoading, setIsMessageLoading] = useState(false);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [syncState, setSyncState] = useState("idle");
  const [isThemeMenuOpen, setIsThemeMenuOpen] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(false);
  const [composeValue, setComposeValue] = useState(null);
  const [pendingSend, setPendingSend] = useState(null);
  const [isSending, setIsSending] = useState(false);
  const [toast, setToast] = useState(null);
  const platform = /Mac|iPhone|iPad/.test(navigator.platform) ? "mac" : "windows";

  const showToast = useCallback((message, tone = "success") => {
    setToast({ message, tone, id: Date.now() });
  }, []);

  useEffect(() => {
    let active = true;
    Promise.all([mailApi.listInbox(50), mailApi.listDrafts()])
      .then(([inbox, localDrafts]) => {
        if (!active) return;
        setMessages(inbox);
        setDrafts(localDrafts);
        if (inbox.length && window.innerWidth >= 720) {
          const first = addFlag(inbox[0], "\\Seen");
          setSelectedUid(first.uid);
          setSelectedMessage(first);
          setMessages((current) =>
            current.map((mail) => (mail.uid === first.uid ? first : mail)),
          );
        }
      })
      .catch((error) => showToast(error?.message || "无法读取本地邮箱", "error"));
    return () => {
      active = false;
    };
  }, [showToast]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    window.localStorage.setItem("mine-mail-theme", theme);
  }, [theme]);

  useEffect(() => {
    if (!toast) return undefined;
    const timer = window.setTimeout(() => setToast(null), 3800);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    const onKeyDown = (event) => {
      if (
        !composeValue &&
        !pendingSend &&
        event.key.toLowerCase() === "n" &&
        !event.metaKey &&
        !event.ctrlKey &&
        !["INPUT", "TEXTAREA"].includes(document.activeElement?.tagName)
      ) {
        event.preventDefault();
        setComposeValue(structuredClone(emptyCompose));
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        document.querySelector(".search-box input")?.focus();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [composeValue, pendingSend]);

  const folderMessages = useMemo(() => {
    if (activeFolder === "inbox") return messages;
    if (activeFolder === "starred") {
      return messages.filter((message) => hasFlag(message, "\\Flagged"));
    }
    if (activeFolder === "drafts") return drafts.map(toDraftMessage);
    if (activeFolder === "sent") return sentMessages;
    return [];
  }, [activeFolder, drafts, messages, sentMessages]);

  const visibleMessages = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return folderMessages.filter((message) => {
      if (filter === "unread" && hasFlag(message, "\\Seen")) return false;
      if (filter === "starred" && !hasFlag(message, "\\Flagged")) return false;
      if (!normalizedQuery) return true;
      return [
        message.subject,
        message.preview,
        message.sender?.name,
        message.sender?.email,
      ].some((value) => value?.toLowerCase().includes(normalizedQuery));
    });
  }, [filter, folderMessages, query]);

  const selectedIndex = visibleMessages.findIndex(
    (message) => message.uid === selectedUid,
  );

  const handleFolderChange = (folder) => {
    setActiveFolder(folder);
    setFilter("all");
    setQuery("");
    setSelectedUid(null);
    setSelectedMessage(null);
    setIsSidebarOpen(false);
  };

  const handleSelect = async (message) => {
    if (message.kind === "draft") {
      setComposeValue({
        to: message.draft.to || [],
        cc: message.draft.cc || [],
        bcc: message.draft.bcc || [],
        subject: message.draft.subject || "",
        body_text: message.draft.body_text || "",
      });
      return;
    }

    const localMessage = addFlag(message, "\\Seen");
    setSelectedUid(message.uid);
    setSelectedMessage(localMessage);
    setMessages((current) =>
      current.map((mail) => (mail.uid === message.uid ? localMessage : mail)),
    );

    if (!message.body_fetched || !message.body_text) {
      setIsMessageLoading(true);
      try {
        const fullMessage = await mailApi.fetchMessage(message.uid);
        if (fullMessage) {
          const readMessage = addFlag(fullMessage, "\\Seen");
          setSelectedMessage(readMessage);
          setMessages((current) =>
            current.map((mail) =>
              mail.uid === fullMessage.uid ? readMessage : mail,
            ),
          );
        }
      } catch (error) {
        showToast(error?.message || "邮件正文加载失败", "error");
      } finally {
        setIsMessageLoading(false);
      }
    }
  };

  const handleToggleStar = (uid) => {
    setMessages((current) =>
      current.map((message) =>
        message.uid === uid ? toggleFlag(message, "\\Flagged") : message,
      ),
    );
    setSelectedMessage((current) =>
      current?.uid === uid ? toggleFlag(current, "\\Flagged") : current,
    );
  };

  const handleSync = async () => {
    setSyncState("syncing");
    try {
      const report = await mailApi.syncInbox(50);
      const inbox = await mailApi.listInbox(50);
      setMessages(inbox);
      setSyncState("done");
      showToast(
        report.fetched
          ? `同步完成，收到 ${report.fetched} 封新邮件`
          : "收件箱已是最新状态",
      );
    } catch (error) {
      setSyncState("error");
      showToast(error?.message || "同步失败，请检查网络", "error");
    }
  };

  const handleSaveDraft = async (request) => {
    try {
      const draft = await mailApi.saveDraft(request);
      setDrafts((current) => [draft, ...current]);
      setComposeValue(null);
      showToast("草稿已保存到本地");
    } catch (error) {
      showToast(error?.message || "草稿保存失败", "error");
    }
  };

  const handleConfirmSend = async () => {
    if (!pendingSend) return;
    setIsSending(true);
    try {
      const result = await mailApi.sendCompose(pendingSend);
      if (result.status !== "sent") {
        const deliveryMessages = {
          retryable: "暂时无法发送，邮件已保留在发件队列",
          rejected: "服务器拒绝了这封邮件，请检查地址或内容",
          delivery_unknown: "投递结果未知，请先到邮箱服务器确认，切勿立即重发",
        };
        setPendingSend(null);
        if (result.status === "delivery_unknown") setComposeValue(null);
        showToast(
          deliveryMessages[result.status] || "邮件尚未发送，已保留在发件队列",
          "error",
        );
        return;
      }
      setSentMessages((current) => [
        {
          id: crypto.randomUUID(),
          uid: `sent-${Date.now()}`,
          subject: pendingSend.subject || "（无主题）",
          sender: { name: "我", email: "" },
          to: pendingSend.to.map((email) => ({ name: null, email })),
          sent_at: new Date().toISOString(),
          flags: ["\\Seen"],
          preview: pendingSend.body_text,
          body_text: pendingSend.body_text,
          attachment_names: [],
          body_fetched: true,
        },
        ...current,
      ]);
      setPendingSend(null);
      setComposeValue(null);
      showToast("邮件已经发送");
    } catch (error) {
      showToast(error?.message || "邮件发送失败，已保留在发件队列", "error");
      setPendingSend(null);
    } finally {
      setIsSending(false);
    }
  };

  const openReply = () => {
    if (!selectedMessage) return;
    setComposeValue({
      to: selectedMessage.sender?.email ? [selectedMessage.sender.email] : [],
      cc: [],
      bcc: [],
      subject: selectedMessage.subject.startsWith("Re:")
        ? selectedMessage.subject
        : `Re: ${selectedMessage.subject}`,
      body_text: `\n\n—— 原邮件 ——\n${selectedMessage.body_text || selectedMessage.preview}`,
    });
  };

  const openForward = () => {
    if (!selectedMessage) return;
    setComposeValue({
      to: [],
      cc: [],
      bcc: [],
      subject: selectedMessage.subject.startsWith("Fwd:")
        ? selectedMessage.subject
        : `Fwd: ${selectedMessage.subject}`,
      body_text: `\n\n—— 转发邮件 ——\n${selectedMessage.body_text || selectedMessage.preview}`,
    });
  };

  const navigateRelative = (offset) => {
    const next = visibleMessages[selectedIndex + offset];
    if (next) handleSelect(next);
  };

  return (
    <div
      className={`app-shell platform-${platform} ${isSidebarOpen ? "sidebar-is-open" : ""} ${selectedMessage ? "has-selection" : ""}`}
      data-runtime={isTauriRuntime ? "tauri" : "web"}
    >
      <div className="app-wallpaper" aria-hidden="true" />

      {!isTauriRuntime ? (
        <div className="web-window-chrome" aria-hidden="true">
          {platform === "mac" ? (
            <div className="traffic-lights">
              <span />
              <span />
              <span />
            </div>
          ) : (
            <div className="window-controls">
              <span><Minus size={14} /></span>
              <span><Square size={12} /></span>
              <span><X size={14} /></span>
            </div>
          )}
        </div>
      ) : null}

      <div className="mail-layout">
        <Sidebar
          activeFolder={activeFolder}
          onFolderChange={handleFolderChange}
          onCompose={() => setComposeValue(structuredClone(emptyCompose))}
          theme={theme}
          onThemeChange={(nextTheme) => {
            setTheme(nextTheme);
            setIsThemeMenuOpen(false);
          }}
          isThemeMenuOpen={isThemeMenuOpen}
          onThemeMenuToggle={() => setIsThemeMenuOpen((open) => !open)}
        />

        {isSidebarOpen ? (
          <button
            className="sidebar-backdrop"
            type="button"
            aria-label="关闭导航"
            onClick={() => setIsSidebarOpen(false)}
          />
        ) : null}

        <MailList
          folderLabel={folderLabels[activeFolder]}
          messages={visibleMessages}
          selectedUid={selectedUid}
          onSelect={handleSelect}
          onToggleStar={handleToggleStar}
          query={query}
          onQueryChange={setQuery}
          filter={filter}
          onFilterChange={setFilter}
          onSync={handleSync}
          syncState={syncState}
          onOpenMobileNav={() => setIsSidebarOpen(true)}
          searchShortcut={platform === "mac" ? "⌘ K" : "Ctrl K"}
        />

        <MessageView
          message={selectedMessage}
          isLoading={isMessageLoading}
          onClose={() => {
            setSelectedUid(null);
            setSelectedMessage(null);
          }}
          onReply={openReply}
          onForward={openForward}
          onPrevious={() => navigateRelative(-1)}
          onNext={() => navigateRelative(1)}
          canPrevious={selectedIndex > 0}
          canNext={selectedIndex >= 0 && selectedIndex < visibleMessages.length - 1}
        />
      </div>

      {composeValue ? (
        <ComposePanel
          initialValue={composeValue}
          isSending={isSending}
          onClose={() => setComposeValue(null)}
          onSaveDraft={handleSaveDraft}
          onRequestSend={(request) => setPendingSend(structuredClone(request))}
          sendShortcut={platform === "mac" ? "⌘ ↵" : "Ctrl ↵"}
        />
      ) : null}

      <SendConfirmDialog
        request={pendingSend}
        isSending={isSending}
        onCancel={() => setPendingSend(null)}
        onConfirm={handleConfirmSend}
      />
      <Toast toast={toast} onClose={() => setToast(null)} />
    </div>
  );
}
