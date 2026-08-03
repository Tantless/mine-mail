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

export function normalizeUpdaterManifest({ manifest, releaseAssets }) {
  if (!manifest || typeof manifest !== "object") {
    throw new Error("latest.json is not a JSON object.");
  }
  if (!Array.isArray(releaseAssets) || releaseAssets.length === 0) {
    throw new Error("The GitHub Release has no downloadable assets.");
  }

  const assetByUrl = new Map();
  for (const asset of releaseAssets) {
    if (!asset?.name || !asset?.browser_download_url) continue;
    const browserDownloadUrl = normalizedUrl(
      asset.browser_download_url,
      `${asset.name} browser download URL`,
    );
    if (!browserDownloadUrl.startsWith("https:")) {
      throw new Error(`${asset.name} browser download URL must use HTTPS.`);
    }
    for (const url of [asset.url, asset.browser_download_url].filter(Boolean)) {
      assetByUrl.set(normalizedUrl(url, `${asset.name} asset URL`), {
        browserDownloadUrl,
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
          url: asset.browserDownloadUrl,
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
  if (!manifestPath || !releaseAssetsPath) {
    throw new Error(
      "Usage: node normalize-updater-manifest.mjs <latest.json> <release-assets.json>",
    );
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const releaseAssets = JSON.parse(
    fs.readFileSync(releaseAssetsPath, "utf8"),
  );
  const normalized = normalizeUpdaterManifest({ manifest, releaseAssets });
  fs.writeFileSync(manifestPath, `${JSON.stringify(normalized, null, 2)}\n`);
  console.log("Normalized updater URLs to GitHub browser download URLs.");
}
