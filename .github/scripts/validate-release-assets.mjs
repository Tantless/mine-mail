import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import { releaseAssetDownloadUrl } from "./normalize-updater-manifest.mjs";

function minisignKeyId(encodedEnvelope, label) {
  const envelope = Buffer.from(
    String(encodedEnvelope).trim(),
    "base64",
  ).toString("utf8");
  const encodedPayload = envelope
    .split(/\r?\n/)
    .find(
      (line) =>
        line &&
        !line.startsWith("untrusted comment") &&
        !line.startsWith("trusted comment"),
    );
  if (!encodedPayload) {
    throw new Error(`${label} is not a valid minisign envelope.`);
  }
  const payload = Buffer.from(encodedPayload, "base64");
  if (payload.length < 10) {
    throw new Error(`${label} does not contain a minisign key id.`);
  }
  return payload.subarray(2, 10).toString("hex");
}

function normalizedUrl(value) {
  return new URL(value).href;
}

export function validateReleaseAssets({
  manifest,
  releaseAssets,
  repository,
  tauriConfig,
  tag,
}) {
  if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag)) {
    throw new Error(`Release tag "${tag}" is invalid.`);
  }
  if (!Array.isArray(releaseAssets) || releaseAssets.length === 0) {
    throw new Error(`Release ${tag} has no downloadable assets.`);
  }

  const expectedVersion = tag.slice(1);
  const expectedAssets = {
    manifest: "latest.json",
    windowsInstaller: `Mine-Mail_${expectedVersion}_x64-setup.exe`,
    macosInstaller: `Mine.Mail_${expectedVersion}_aarch64.dmg`,
    macosUpdater: `Mine.Mail_${expectedVersion}_aarch64.app.tar.gz`,
    linuxDeb: `Mine.Mail_${expectedVersion}_amd64.deb`,
    linuxAppImage: `Mine.Mail_${expectedVersion}_amd64.AppImage`,
  };
  const expectedAssetNames = new Set(Object.values(expectedAssets));
  const assetNames = releaseAssets.map((asset) => asset.name);
  const missingAssets = [...expectedAssetNames].filter(
    (name) => !assetNames.includes(name),
  );
  const unsupportedAssets = assetNames.filter(
    (name) => !expectedAssetNames.has(name),
  );
  if (missingAssets.length > 0) {
    throw new Error(
      `Release ${tag} is missing required assets: ${missingAssets.join(", ")}.`,
    );
  }
  if (unsupportedAssets.length > 0) {
    throw new Error(
      `Release ${tag} contains unsupported assets: ${unsupportedAssets.join(", ")}.`,
    );
  }

  const assetNameByUrl = new Map();
  const stableUrlByAssetName = new Map();
  for (const asset of releaseAssets) {
    const stableDownloadUrl = releaseAssetDownloadUrl({
      repository,
      tag,
      assetName: asset.name,
    });
    for (const url of [
      asset.url,
      asset.browser_download_url,
      stableDownloadUrl,
    ].filter(Boolean)) {
      assetNameByUrl.set(normalizedUrl(url), asset.name);
    }
    stableUrlByAssetName.set(asset.name, stableDownloadUrl);
  }

  const requiredPlatformAssets = {
    "darwin-aarch64": expectedAssets.macosUpdater,
    "darwin-aarch64-app": expectedAssets.macosUpdater,
    "linux-x86_64": expectedAssets.linuxAppImage,
    "linux-x86_64-appimage": expectedAssets.linuxAppImage,
    "linux-x86_64-deb": expectedAssets.linuxDeb,
    "windows-x86_64": expectedAssets.windowsInstaller,
    "windows-x86_64-nsis": expectedAssets.windowsInstaller,
  };
  const supportedPlatforms = new Set(Object.keys(requiredPlatformAssets));
  const unsupportedPlatforms = Object.keys(manifest.platforms ?? {}).filter(
    (platform) => !supportedPlatforms.has(platform),
  );
  if (unsupportedPlatforms.length > 0) {
    throw new Error(
      `latest.json contains unsupported updater platforms: ${unsupportedPlatforms.join(", ")}.`,
    );
  }

  if (String(manifest.version).replace(/^v/, "") !== expectedVersion) {
    throw new Error(
      `latest.json version ${manifest.version} does not match ${tag}.`,
    );
  }

  const embeddedKeyId = minisignKeyId(
    tauriConfig.plugins?.updater?.pubkey,
    "embedded updater public key",
  );
  for (const [platform, expectedAsset] of Object.entries(
    requiredPlatformAssets,
  )) {
    const entry = manifest.platforms?.[platform];
    if (!entry?.url || !entry?.signature) {
      throw new Error(`latest.json is missing ${platform}.`);
    }
    const url = normalizedUrl(entry.url);
    const referencedAsset = assetNameByUrl.get(url);
    if (!url.startsWith("https:") || !referencedAsset) {
      throw new Error(
        `${platform} update URL does not belong to release ${tag}.`,
      );
    }
    if (referencedAsset !== expectedAsset) {
      throw new Error(
        `${platform} must reference ${expectedAsset}, not ${referencedAsset}.`,
      );
    }
    if (url !== stableUrlByAssetName.get(expectedAsset)) {
      throw new Error(
        `${platform} must use the version-pinned GitHub download URL for ${expectedAsset}.`,
      );
    }
    const signatureKeyId = minisignKeyId(
      entry.signature,
      `${platform} updater signature`,
    );
    if (signatureKeyId !== embeddedKeyId) {
      throw new Error(
        `${platform} updater signature does not match the embedded public key.`,
      );
    }
  }

  return `Validated the ${tag} release asset and updater matrix.`;
}

const isMain =
  process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1]);
if (isMain) {
  const [manifestPath, releaseAssetsPath] = process.argv.slice(2);
  if (!manifestPath || !releaseAssetsPath) {
    throw new Error(
      "Usage: node validate-release-assets.mjs <latest.json> <release-assets.json>",
    );
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const releaseAssets = JSON.parse(
    fs.readFileSync(releaseAssetsPath, "utf8"),
  );
  const tauriConfig = JSON.parse(
    fs.readFileSync("web/src-tauri/tauri.conf.json", "utf8"),
  );
  console.log(
    validateReleaseAssets({
      manifest,
      releaseAssets,
      repository: process.env.GITHUB_REPOSITORY,
      tauriConfig,
      tag: process.env.RELEASE_TAG,
    }),
  );
}
