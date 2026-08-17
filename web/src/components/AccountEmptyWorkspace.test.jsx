import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("./ReaderIdleExperience.jsx", () => ({
  ReaderIdleExperience: () => <div data-testid="reader-idle-experience" />,
}));

import { AccountEmptyWorkspace } from "./AccountEmptyWorkspace.jsx";

describe("account empty workspace", () => {
  afterEach(cleanup);

  it("shows the idle poetry by default", () => {
    render(<AccountEmptyWorkspace onConnect={() => {}} />);

    expect(screen.getByTestId("reader-idle-experience")).toBeTruthy();
    expect(screen.getByRole("button", { name: "连接邮箱" })).toBeTruthy();
  });

  it("keeps the account prompt when idle poetry is disabled", () => {
    render(
      <AccountEmptyWorkspace
        showIdlePoetry={false}
        needsRepair
        onConnect={() => {}}
      />,
    );

    expect(screen.queryByTestId("reader-idle-experience")).toBeNull();
    expect(screen.getByText("账户需要重新连接")).toBeTruthy();
    expect(screen.getByRole("button", { name: "修复账户" })).toBeTruthy();
  });
});
