import { describe, expect, it } from "vitest";
import { messageNavigationKey } from "./messageNavigation.js";

describe("messageNavigationKey", () => {
  it("uses only the opaque local message id", () => {
    expect(
      messageNavigationKey({
        id: "account-bound:mail-7",
        mailbox: "INBOX",
        uid: 7,
      }),
    ).toBe("message:account-bound:mail-7");
  });

  it("rejects provider mailbox and UID coordinates even for contact history", () => {
    expect(messageNavigationKey({ mailbox: "INBOX", uid: 7 })).toBeNull();
    expect(
      messageNavigationKey({
        contactHistory: true,
        mailbox: "Archive/2026",
        uid: 7,
      }),
    ).toBeNull();
  });

  it("rejects numeric SQLite row ids", () => {
    expect(messageNavigationKey({ id: 7 })).toBeNull();
  });
});
