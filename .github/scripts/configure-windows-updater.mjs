import fs from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { releaseAssetDownloadUrl } from "./normalize-updater-manifest.mjs";

const WINDOWS_PLATFORMS = ["windows-x86_64", "windows-x86_64-nsis"];

function normalizedUrl(value, label) {
  try {
    return new URL(value).href;
  } catch {
    throw new Error(`${label} is not a valid URL.`);
  }
}

function assetUrls({ asset, repository, tag }) {
  return [
    asset.url,
    asset.browser_download_url,
    releaseAssetDownloadUrl({ repository, tag, assetName: asset.name }),
  ]
    .filter(Boolean)
    .map((url) => normalizedUrl(url, `${asset.name} asset URL`));
}

export function configureWindowsUpdater({
  manifest,
  releaseAssets,
  repository,
  tag,
}) {
  if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag)) {
    throw new Error(`Release tag "${tag}" is invalid.`);
  }
  if (!manifest || typeof manifest !== "object") {
    throw new Error("latest.json is not a JSON object.");
  }
  if (!Array.isArray(releaseAssets) || releaseAssets.length === 0) {
    throw new Error(`Release ${tag} has no downloadable assets.`);
  }

  const version = tag.slice(1);
  const generatedName = `Mine.Mail_${version}_x64-setup.exe`;
  const updaterName = `Mine-Mail_${version}_windows-x64-updater.exe`;
  const updaterAsset = releaseAssets.find((asset) => asset?.name === updaterName);
  if (!updaterAsset) {
    throw new Error(`Release ${tag} is missing ${updaterName}.`);
  }

  const allowedSourceAssets = releaseAssets.filter(
    (asset) => asset?.name === generatedName || asset?.name === updaterName,
  );
  const allowedSourceUrls = new Set(
    allowedSourceAssets.flatMap((asset) =>
      assetUrls({ asset, repository, tag }),
    ),
  );
  const updaterUrl = releaseAssetDownloadUrl({
    repository,
    tag,
    assetName: updaterName,
  });

  const platforms = { ...manifest.platforms };
  for (const platform of WINDOWS_PLATFORMS) {
    const entry = manifest.platforms?.[platform];
    if (!entry?.url || !entry?.signature) {
      throw new Error(`latest.json is missing ${platform}.`);
    }
    const currentUrl = normalizedUrl(entry.url, `${platform} update URL`);
    if (!allowedSourceUrls.has(currentUrl)) {
      throw new Error(
        `${platform} must originate from the generated or dedicated Windows updater asset.`,
      );
    }
    platforms[platform] = { ...entry, url: updaterUrl };
  }

  return { ...manifest, platforms };
}

const isMain =
  process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1]);
if (isMain) {
  const [manifestPath, releaseAssetsPath] = process.argv.slice(2);
  if (!manifestPath || !releaseAssetsPath) {
    throw new Error(
      "Usage: node configure-windows-updater.mjs <latest.json> <release-assets.json>",
    );
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const releaseAssets = JSON.parse(
    fs.readFileSync(releaseAssetsPath, "utf8"),
  );
  const configured = configureWindowsUpdater({
    manifest,
    releaseAssets,
    repository: process.env.GITHUB_REPOSITORY,
    tag: process.env.RELEASE_TAG,
  });
  fs.writeFileSync(manifestPath, `${JSON.stringify(configured, null, 2)}\n`);
  console.log("Configured Windows updates to use the dedicated Tauri package.");
}
