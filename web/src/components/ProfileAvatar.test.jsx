import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  EditableProfileAvatar,
  ProfileAvatar,
  avatarToneForEmail,
  trustedBrandForEmail,
} from "./ProfileAvatar.jsx";
import { brandRules } from "./brandAvatars.js";

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
    expect(trustedBrandForEmail("notice@qq.com")?.id).toBe("tencentqq");
    expect(trustedBrandForEmail("updates@email.openai.com")?.id).toBe("openai");
    expect(trustedBrandForEmail("news@figma.com")?.id).toBe("figma");
    expect(trustedBrandForEmail("hello@unity3d.com")?.id).toBe("unity");
    expect(trustedBrandForEmail("updates@openrouter.ai")?.id).toBe(
      "openrouter",
    );
    expect(trustedBrandForEmail("hello@getzep.com")?.id).toBe("zep");
    expect(trustedBrandForEmail("notice@sc.mail.deepseek.com")?.id).toBe(
      "deepseek",
    );
    expect(trustedBrandForEmail("fake@github.com.example.org")).toBeNull();
    expect(trustedBrandForEmail("fake@c-openai.com")).toBeNull();
  });

  it("renders bundled vector marks for recognized brands", () => {
    const { container, rerender } = render(
      <ProfileAvatar email="news@figma.com" label="Figma" />,
    );

    expect(
      container.querySelectorAll(".profile-avatar--figma svg path"),
    ).toHaveLength(5);
    expect(screen.getByLabelText("Figma")).toBeTruthy();

    rerender(<ProfileAvatar email="updates@email.openai.com" label="OpenAI" />);
    expect(
      container.querySelector(".profile-avatar--openai .brand-mark__icon--original"),
    ).toBeTruthy();
    expect(screen.getByLabelText("OpenAI")).toBeTruthy();

    rerender(<ProfileAvatar email="security@accounts.google.com" label="Google" />);
    expect(
      Array.from(
        container.querySelectorAll(".profile-avatar--google svg path"),
        (path) => path.getAttribute("fill"),
      ),
    ).toEqual(["#4285f4", "#34a853", "#fbbc05", "#eb4335"]);

    rerender(<ProfileAvatar email="notice@qq.com" label="QQ 邮箱" />);
    expect(container.querySelector(".profile-avatar--tencentqq svg path")).toBeTruthy();
    expect(container.querySelector(".profile-avatar--tencentqq .brand-mark__letters")).toBeNull();
    expect(screen.getByLabelText("腾讯 QQ")).toBeTruthy();
  });

  it("keeps the built-in brand registry complete and unambiguous", () => {
    const ids = brandRules.map((brand) => brand.id);
    const domains = brandRules.flatMap((brand) => brand.domains);

    expect(new Set(ids).size).toBe(ids.length);
    expect(new Set(domains).size).toBe(domains.length);
    expect(brandRules.length).toBeGreaterThan(60);
    for (const brand of brandRules) {
      expect(brand.domains.length).toBeGreaterThan(0);
      expect(brand.background).toMatch(/^#[\da-f]{3,6}$/i);
      expect(brand.foreground).toMatch(/^#[\da-f]{3,6}$/i);
      expect(
        Boolean(
          brand.originalMark ||
            brand.simpleIcon ||
            brand.Icon ||
            brand.mark ||
            brand.letters,
        ),
      ).toBe(true);
    }
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
