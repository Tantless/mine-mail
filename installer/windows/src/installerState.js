export const INSTALLER_STEPS = [
  { id: "ready", label: "准备" },
  { id: "installing", label: "安装" },
  { id: "success", label: "完成" },
];

export function activeStepIndex(state) {
  if (state === "success") return 2;
  if (state === "installing" || state === "error") return 1;
  return 0;
}

export function stepTone(state, index) {
  const activeIndex = activeStepIndex(state);
  if (index < activeIndex) return "done";
  if (index === activeIndex) return state === "error" ? "error" : "active";
  return "waiting";
}

export function defaultPreviewInfo() {
  return {
    version: "0.1.1",
    defaultInstallDir: "C:\\Users\\You\\AppData\\Local\\Mine Mail",
    payloadAvailable: true,
  };
}
