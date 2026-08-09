import { readFile, readdir, stat } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const distUrl = new URL("../dist/", import.meta.url);
const assetsUrl = new URL("assets/", distUrl);
const indexHtml = await readFile(new URL("index.html", distUrl), "utf8");
const assetNames = await readdir(assetsUrl);
const javascriptAssets = assetNames.filter((name) => name.endsWith(".js"));

if (javascriptAssets.length === 0) {
  throw new Error("生产构建中没有可检查的 JavaScript 产物。");
}

const assetSources = await Promise.all(
  javascriptAssets.map(async (name) => ({
    name,
    source: await readFile(new URL(name, assetsUrl), "utf8"),
    size: (await stat(new URL(name, assetsUrl))).size,
  })),
);

const forbiddenProductionMarkers = [
  "demo@163.com",
  "demo-primary",
  "demo-message-01",
  "欢迎来到 Mine Mail",
  "这是第一版原型",
  "尚未实现",
  "即将支持",
  "功能开发中",
  "演示账户不存在",
  "分页游标无效或已过期",
  "此邮箱角色当前不可同步",
  "当前邮箱中不能执行此操作",
  "找不到要回复的邮件",
  "找不到要添加附件的草稿",
  "demo-page-",
];

const leakedMarkers = forbiddenProductionMarkers.flatMap((marker) =>
  assetSources
    .filter(({ source }) => source.includes(marker))
    .map(({ name }) => `${marker} -> ${name}`),
);
if (leakedMarkers.length > 0) {
  throw new Error(
    `生产包包含 demo 实现、夹具或占位文案：\n${leakedMarkers.join("\n")}`,
  );
}

const oversizedChunks = assetSources.filter(({ size }) => size > 500 * 1024);
if (oversizedChunks.length > 0) {
  throw new Error(
    `生产 JavaScript chunk 超过 500 KiB：\n${oversizedChunks
      .map(({ name, size }) => `${name}: ${(size / 1024).toFixed(2)} KiB`)
      .join("\n")}`,
  );
}

const entryMatch = indexHtml.match(
  /<script[^>]+type="module"[^>]+src="([^"]+)"[^>]*>/,
);
if (!entryMatch) {
  throw new Error("无法从生产 index.html 确认入口脚本。");
}
const entryName = fileURLToPath(new URL(entryMatch[1], distUrl))
  .replaceAll("\\", "/")
  .split("/")
  .at(-1);
const entry = assetSources.find(({ name }) => name === entryName);
if (!entry) {
  throw new Error(`生产入口脚本不存在：${entryName}`);
}

for (const prefix of [
  "brand-icons-",
  "ContactsWorkspace-",
  "ComposeAiMarkdown-",
  "SettingsPanel-",
  "RichTextEditor-",
  "react-runtime-",
]) {
  if (!javascriptAssets.some((name) => name.startsWith(prefix))) {
    throw new Error(`缺少预期分包：${prefix}`);
  }
}

console.log(
  `生产包边界检查通过：入口 ${(entry.size / 1024).toFixed(2)} KiB，` +
    `${javascriptAssets.length} 个 JavaScript chunk，无 demo 实现、夹具或占位文案。`,
);
