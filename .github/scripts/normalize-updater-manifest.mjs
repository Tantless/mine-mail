import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

function normalizedUrl(value, label) {
  try {
    return new URL(value).href;
  } catch {
    throw new Error(`${label} is not a valid URL.`);
  }
}

export function releaseAssetDownloadUrl({ repository, tag, assetName }) {
  if (!/^[0-9A-Za-z_.-]+\/[0-9A-Za-z_.-]+$/.test(repository || "")) {
    throw new Error("GITHUB_REPOSITORY must use the owner/repository format.");
  }
  if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag || "")) {
    throw new Error(`Release tag "${tag}" is invalid.`);
  }
  if (!assetName || /[\\/]/.test(assetName)) {
    throw new Error("Release asset name is invalid.");
  }
  return new URL(
    `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(assetName)}`,
  ).href;
}

export function normalizeUpdaterManifest({
  manifest,
  releaseAssets,
  repository,
  tag,
}) {
  if (!manifest || typeof manifest !== "object") {
    throw new Error("latest.json is not a JSON object.");
  }
  if (!Array.isArray(releaseAssets) || releaseAssets.length === 0) {
    throw new Error("The GitHub Release has no downloadable assets.");
  }

  const assetByUrl = new Map();
  for (const asset of releaseAssets) {
    if (!asset?.name) continue;
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
      assetByUrl.set(normalizedUrl(url, `${asset.name} asset URL`), {
        stableDownloadUrl,
      });
    }
  }

  const platforms = Object.fromEntries(
    Object.entries(manifest.platforms ?? {}).map(([platform, entry]) => {
      if (!entry?.url) return [platform, entry];
      const currentUrl = normalizedUrl(entry.url, `${platform} update URL`);
      const asset = assetByUrl.get(currentUrl);
      if (!asset) {
        throw new Error(
          `${platform} update URL does not belong to this GitHub Release.`,
        );
      }
      return [
        platform,
        {
          ...entry,
          url: asset.stableDownloadUrl,
        },
      ];
    }),
  );

  return { ...manifest, platforms };
}

const isMain =
  process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1]);
if (isMain) {
  const [manifestPath, releaseAssetsPath] = process.argv.slice(2);
  const repository = process.env.GITHUB_REPOSITORY;
  const tag = process.env.RELEASE_TAG;
  if (!manifestPath || !releaseAssetsPath || !repository || !tag) {
    throw new Error(
      "Usage: GITHUB_REPOSITORY=owner/repository RELEASE_TAG=v1.2.3 node normalize-updater-manifest.mjs <latest.json> <release-assets.json>",
    );
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const releaseAssets = JSON.parse(
    fs.readFileSync(releaseAssetsPath, "utf8"),
  );
  const normalized = normalizeUpdaterManifest({
    manifest,
    releaseAssets,
    repository,
    tag,
  });
  fs.writeFileSync(manifestPath, `${JSON.stringify(normalized, null, 2)}\n`);
  console.log("Normalized updater URLs to version-pinned GitHub download URLs.");
}
