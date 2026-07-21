import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  EditableProfileAvatar,
  ProfileAvatar,
  avatarToneForEmail,
  trustedBrandForEmail,
} from "./ProfileAvatar.jsx";

describe("ProfileAvatar", () => {
  afterEach(() => cleanup());

  it("keeps the fallback tone stable for a normalized email", () => {
    expect(avatarToneForEmail("  Friend@Example.COM ")).toBe(
      avatarToneForEmail("friend@example.com"),
    );

    const { rerender } = render(
      <ProfileAvatar email="Friend@Example.COM" label="Friend" />,
    );
    const firstClassName = screen.getByText("FR").className;

    rerender(<ProfileAvatar email=" friend@example.com " label="Friend" />);
    expect(screen.getByText("FR").className).toBe(firstClassName);
    expect(firstClassName).toMatch(/profile-avatar--tone-[0-3]/);
  });

  it("matches only trusted domain boundaries", () => {
    expect(trustedBrandForEmail("notifications@github.com")?.id).toBe("github");
    expect(trustedBrandForEmail("security@accounts.google.com")?.id).toBe("google");
    expect(trustedBrandForEmail("notice@163.com")?.id).toBe("netease");
    expect(trustedBrandForEmail("fake@github.com.example.org")).toBeNull();
  });

  it("prefers a local custom avatar over a trusted brand", () => {
    const { container } = render(
      <ProfileAvatar
        email="notifications@github.com"
        label="GitHub"
        customSrc="data:image/png;base64,AQID"
      />,
    );

    expect(container.querySelector("img")?.getAttribute("src")).toBe(
      "data:image/png;base64,AQID",
    );
    expect(container.querySelector(".profile-avatar--github")).toBeNull();
  });

  it("offers explicit replace and remove controls for a local avatar", () => {
    const onSelectFile = vi.fn();
    const onRemove = vi.fn();
    render(
      <EditableProfileAvatar
        email="friend@example.com"
        label="Friend"
        customSrc="data:image/png;base64,AQID"
        onSelectFile={onSelectFile}
        onRemove={onRemove}
      />,
    );
    const file = new File([new Uint8Array([1, 2, 3])], "avatar.png", {
      type: "image/png",
    });

    fireEvent.change(screen.getByLabelText("设置 Friend 的头像"), {
      target: { files: [file] },
    });
    fireEvent.click(screen.getByRole("button", { name: "移除 Friend 的自定义头像" }));

    expect(onSelectFile).toHaveBeenCalledWith(file);
    expect(onRemove).toHaveBeenCalledOnce();
  });
});
