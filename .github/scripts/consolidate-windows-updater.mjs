import fs from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { releaseAssetDownloadUrl } from "./normalize-updater-manifest.mjs";

const WINDOWS_PLATFORMS = ["windows-x86_64", "windows-x86_64-nsis"];

function normalizedUrl(value) {
  return new URL(value).href;
}

export function consolidateWindowsUpdater({
  manifest,
  releaseAssets,
  repository,
  signature,
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
  const installerName = `Mine-Mail_${version}_x64-setup.exe`;
  if (!releaseAssets.some((asset) => asset?.name === installerName)) {
    throw new Error(`Release ${tag} is missing ${installerName}.`);
  }

  const installerUrl = releaseAssetDownloadUrl({
    repository,
    tag,
    assetName: installerName,
  });
  const windowsEntries = WINDOWS_PLATFORMS.map((platform) => {
    const entry = manifest.platforms?.[platform];
    if (!entry?.url || !entry?.signature) {
      throw new Error(`latest.json is missing ${platform}.`);
    }
    return [platform, entry];
  });

  let updaterSignature = String(signature ?? "").trim();
  if (!updaterSignature) {
    const alreadyConsolidated = windowsEntries.every(
      ([, entry]) => normalizedUrl(entry.url) === installerUrl,
    );
    const existingSignatures = new Set(
      windowsEntries
        .map(([, entry]) => String(entry.signature).trim())
        .filter(Boolean),
    );
    if (!alreadyConsolidated || existingSignatures.size !== 1) {
      throw new Error(
        `Release ${tag} needs the branded Windows updater signature before it can be published.`,
      );
    }
    [updaterSignature] = existingSignatures;
  }

  const platforms = { ...manifest.platforms };
  for (const [platform, entry] of windowsEntries) {
    platforms[platform] = {
      ...entry,
      url: installerUrl,
      signature: updaterSignature,
    };
  }
  return { ...manifest, platforms };
}

const isMain =
  process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1]);
if (isMain) {
  const [manifestPath, releaseAssetsPath, signaturePath] = process.argv.slice(2);
  if (!manifestPath || !releaseAssetsPath) {
    throw new Error(
      "Usage: node consolidate-windows-updater.mjs <latest.json> <release-assets.json> [signature]",
    );
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const releaseAssets = JSON.parse(
    fs.readFileSync(releaseAssetsPath, "utf8"),
  );
  const signature =
    signaturePath && fs.existsSync(signaturePath)
      ? fs.readFileSync(signaturePath, "utf8")
      : undefined;
  const consolidated = consolidateWindowsUpdater({
    manifest,
    releaseAssets,
    repository: process.env.GITHUB_REPOSITORY,
    signature,
    tag: process.env.RELEASE_TAG,
  });
  fs.writeFileSync(manifestPath, `${JSON.stringify(consolidated, null, 2)}\n`);
  console.log("Consolidated Windows installer and updater into one release asset.");
}
