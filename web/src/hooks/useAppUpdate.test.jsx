import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { UpdateProgressNotice } from "../components/UpdateProgressNotice.jsx";
import { useAppUpdate } from "./useAppUpdate.js";

function UpdateHarness({ client }) {
  const update = useAppUpdate(client);
  const [settingsOpen, setSettingsOpen] = useState(true);
  return (
    <>
      {settingsOpen ? (
        <section aria-label="更新设置">
          <button type="button" onClick={() => void update.checkForUpdate()}>
            检查更新
          </button>
          {update.availableUpdate && update.isDialogOpen ? (
            <div role="dialog" aria-label="可用更新">
              <button
                type="button"
                onClick={() => void update.installAvailableUpdate()}
              >
                下载并安装
              </button>
              <button type="button" onClick={update.minimizeDialog}>
                收起
              </button>
            </div>
          ) : null}
          <button type="button" onClick={() => setSettingsOpen(false)}>
            离开设置
          </button>
        </section>
      ) : (
        <button type="button" onClick={() => setSettingsOpen(true)}>
          打开设置
        </button>
      )}
      {update.isDownloadActive &&
      (!settingsOpen || !update.isDialogOpen) ? (
        <UpdateProgressNotice
          version={update.availableUpdate?.version}
          progress={update.progress}
          isCancelling={update.status === "cancelling"}
          canCancel={update.isDownloadCancellable}
          onCancel={() => void update.cancelDownload()}
        />
      ) : null}
      <output>{update.message}</output>
    </>
  );
}

describe("useAppUpdate", () => {
  it("keeps a minimized download alive and cancels only from the stop icon", async () => {
    let rejectInstall;
    const installUpdate = vi.fn(async (_candidate, onEvent) => {
      onEvent({ event: "Started", data: { contentLength: 100 } });
      onEvent({ event: "Progress", data: { chunkLength: 40 } });
      await new Promise((_resolve, reject) => {
        rejectInstall = reject;
      });
    });
    const cancelUpdate = vi.fn(async () => {
      const error = new Error("cancelled");
      error.name = "AppUpdateCancelledError";
      rejectInstall(error);
      return true;
    });
    const client = {
      isSupported: true,
      bundledVersion: "1.1.0",
      getCurrentVersion: vi.fn().mockResolvedValue("1.1.0"),
      checkForUpdate: vi.fn().mockResolvedValue({
        status: "available",
        currentVersion: "1.1.0",
        version: "1.2.0",
        notes: null,
      }),
      installUpdate,
      cancelUpdate,
    };
    const user = userEvent.setup();
    render(<UpdateHarness client={client} />);

    await user.click(screen.getByRole("button", { name: "检查更新" }));
    await user.click(
      await screen.findByRole("button", { name: "下载并安装" }),
    );
    await user.click(screen.getByRole("button", { name: "离开设置" }));

    const notice = await screen.findByRole("status", {
      name: "下载更新 v1.2.0",
    });
    expect(notice.textContent).toContain("下载更新 v1.2.0");
    expect(
      screen.getByRole("progressbar", { name: "更新下载进度" }).value,
    ).toBe(40);
    expect(installUpdate).toHaveBeenCalledOnce();
    expect(cancelUpdate).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "取消更新下载" }));
    expect(cancelUpdate).toHaveBeenCalledOnce();
    await waitFor(() =>
      expect(
        screen.queryByRole("status", { name: "下载更新 v1.2.0" }),
      ).toBeNull(),
    );
    expect(screen.getByText("已取消 v1.2.0 更新下载。")).toBeTruthy();

  });
});
