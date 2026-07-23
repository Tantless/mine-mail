import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  rankRecipientContacts,
  RecipientInput,
} from "./RecipientInput.jsx";

const contacts = [
  {
    email: "recent@example.com",
    displayName: "最近联系人",
    isFavorite: false,
    messageCount: 8,
    lastMessageAt: "2026-07-22T10:00:00Z",
  },
  {
    email: "favorite-two@example.com",
    displayName: "收藏乙",
    isFavorite: true,
    messageCount: 2,
    lastMessageAt: "2026-07-20T10:00:00Z",
  },
  {
    email: "nora@example.com",
    displayName: "Nora",
    isFavorite: false,
    messageCount: 4,
    lastMessageAt: "2026-07-21T10:00:00Z",
  },
  {
    email: "favorite-one@example.com",
    displayName: "收藏甲",
    isFavorite: true,
    messageCount: 1,
    lastMessageAt: "2026-07-21T10:00:00Z",
  },
  {
    email: "chenyu@example.com",
    displayName: "陈屿",
    isFavorite: false,
    messageCount: 3,
    lastMessageAt: "2026-07-19T10:00:00Z",
  },
  {
    email: "linxia@example.com",
    displayName: "林夏",
    isFavorite: false,
    messageCount: 6,
    lastMessageAt: "2026-07-18T10:00:00Z",
  },
  {
    email: "weekly@example.com",
    displayName: "Design Weekly",
    isFavorite: false,
    messageCount: 1,
    lastMessageAt: "2026-07-17T10:00:00Z",
  },
];

function RecipientHarness({ initialRecipients = [], onRecipientsChange = vi.fn() }) {
  const [recipients, setRecipients] = useState(initialRecipients);
  return (
    <RecipientInput
      id="test-recipient"
      label="收件人"
      recipients={recipients}
      contacts={contacts}
      onChange={(next) => {
        setRecipients(next);
        onRecipientsChange(next);
      }}
    />
  );
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("RecipientInput", () => {
  it("pins favorites, shows five contacts first, and expands into a scrollable list", async () => {
    const user = userEvent.setup();
    render(<RecipientHarness />);

    await user.click(screen.getByRole("combobox", { name: "收件人" }));
    const initialOptions = screen.getAllByRole("option");
    expect(initialOptions).toHaveLength(5);
    expect(initialOptions[0].textContent).toContain("收藏甲");
    expect(initialOptions[1].textContent).toContain("收藏乙");
    expect(initialOptions[0].dataset.favorite).toBe("true");

    await user.click(screen.getByRole("button", { name: /显示更多联系人/ }));
    expect(screen.getAllByRole("option")).toHaveLength(7);
    expect(screen.getByRole("listbox").dataset.expanded).toBe("true");
  });

  it("filters by name or email and selects a contact into an avatar token", async () => {
    const user = userEvent.setup();
    const onRecipientsChange = vi.fn();
    render(<RecipientHarness onRecipientsChange={onRecipientsChange} />);
    const input = screen.getByRole("combobox", { name: "收件人" });

    await user.type(input, "nora");
    const option = screen.getByRole("option");
    expect(option.textContent).toContain("nora@example.com");
    fireEvent.pointerEnter(option);
    expect(option.dataset.active).toBe("true");
    await user.click(option);

    expect(onRecipientsChange).toHaveBeenLastCalledWith(["nora@example.com"]);
    expect(screen.getByText("nora@example.com").closest(".recipient-token")).toBeTruthy();
    expect(document.querySelector(".recipient-token__avatar")).toBeTruthy();
  });

  it("turns a manually confirmed email into a token and removes it with Backspace", async () => {
    const user = userEvent.setup();
    const onRecipientsChange = vi.fn();
    render(<RecipientHarness onRecipientsChange={onRecipientsChange} />);
    const input = screen.getByRole("combobox", { name: "收件人" });

    expect(input.autocomplete).toBe("off");
    await user.type(input, "manual@example.com{enter}");
    expect(onRecipientsChange).toHaveBeenLastCalledWith(["manual@example.com"]);
    expect(input.value).toBe("");
    expect(screen.getByRole("button", { name: "移除收件人 manual@example.com" })).toBeTruthy();

    await user.type(input, "{backspace}");
    expect(onRecipientsChange).toHaveBeenLastCalledWith([]);
  });

  it("ranks matching favorites before more recent non-favorites", () => {
    const ranked = rankRecipientContacts(contacts, "example", []);
    expect(ranked.slice(0, 2).every((contact) => contact.isFavorite)).toBe(true);
    expect(ranked[2].email).toBe("recent@example.com");
  });

  it("keeps the suggestions anchored while the compose window enters", () => {
    let anchorTop = 120;
    const frames = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frames.push(callback);
      return frames.length;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function getBoundingClientRect() {
        if (this.classList.contains("recipient-input")) {
          return {
            bottom: anchorTop + 36,
            height: 36,
            left: 100,
            right: 500,
            top: anchorTop,
            width: 400,
            x: 100,
            y: anchorTop,
            toJSON: () => {},
          };
        }
        if (this.classList.contains("recipient-suggestions")) {
          return {
            bottom: 322,
            height: 322,
            left: 0,
            right: 400,
            top: 0,
            width: 400,
            x: 0,
            y: 0,
            toJSON: () => {},
          };
        }
        return {
          bottom: 0,
          height: 0,
          left: 0,
          right: 0,
          top: 0,
          width: 0,
          x: 0,
          y: 0,
          toJSON: () => {},
        };
      },
    );

    render(
      <div className="compose-panel">
        <RecipientHarness />
      </div>,
    );
    fireEvent.focus(screen.getByRole("combobox", { name: "收件人" }));
    const popup = document.querySelector(".recipient-suggestions");
    expect(popup.style.top).toBe("163px");

    anchorTop = 76;
    act(() => {
      frames.shift()(window.performance.now());
    });
    expect(popup.style.top).toBe("119px");
  });
});
