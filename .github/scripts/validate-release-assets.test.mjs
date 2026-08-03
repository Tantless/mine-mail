import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";

import { normalizeUpdaterManifest } from "./normalize-updater-manifest.mjs";
import { validateReleaseAssets } from "./validate-release-assets.mjs";

const tag = "v1.2.3";
const version = tag.slice(1);
const keyId = "0123456789abcdef";

function minisignEnvelope() {
  const payload = Buffer.alloc(10);
  Buffer.from(keyId, "hex").copy(payload, 2);
  return Buffer.from(
    `untrusted comment: test envelope\n${payload.toString("base64")}\n`,
  ).toString("base64");
}

function releaseFixture() {
  const names = [
    "latest.json",
    `Mine-Mail_${version}_x64-setup.exe`,
    `Mine.Mail_${version}_x64-setup.exe`,
    `Mine.Mail_${version}_aarch64.dmg`,
    `Mine.Mail_${version}_aarch64.app.tar.gz`,
    `Mine.Mail_${version}_amd64.deb`,
    `Mine.Mail_${version}_amd64.AppImage`,
  ];
  const releaseAssets = names.map((name, index) => ({
    name,
    url: `https://api.github.com/repos/example/mine-mail/releases/assets/${index + 1}`,
    browser_download_url: `https://github.com/example/mine-mail/releases/download/${tag}/${name}`,
  }));
  const urlFor = (name) =>
    releaseAssets.find((asset) => asset.name === name).browser_download_url;
  const signature = minisignEnvelope();
  const platformAssets = {
    "darwin-aarch64": `Mine.Mail_${version}_aarch64.app.tar.gz`,
    "darwin-aarch64-app": `Mine.Mail_${version}_aarch64.app.tar.gz`,
    "linux-x86_64": `Mine.Mail_${version}_amd64.AppImage`,
    "linux-x86_64-appimage": `Mine.Mail_${version}_amd64.AppImage`,
    "linux-x86_64-deb": `Mine.Mail_${version}_amd64.deb`,
    "windows-x86_64": `Mine.Mail_${version}_x64-setup.exe`,
    "windows-x86_64-nsis": `Mine.Mail_${version}_x64-setup.exe`,
  };
  const platforms = Object.fromEntries(
    Object.entries(platformAssets).map(([platform, name]) => [
      platform,
      { url: urlFor(name), signature },
    ]),
  );
  return {
    manifest: { version, platforms },
    releaseAssets,
    tauriConfig: {
      plugins: { updater: { pubkey: minisignEnvelope() } },
    },
    tag,
  };
}

test("accepts the supported release asset and updater matrix", () => {
  assert.equal(
    validateReleaseAssets(releaseFixture()),
    `Validated the ${tag} release asset and updater matrix.`,
  );
});

test("normalizes updater API asset URLs to browser download URLs", () => {
  const fixture = releaseFixture();
  for (const entry of Object.values(fixture.manifest.platforms)) {
    const asset = fixture.releaseAssets.find(
      (candidate) => candidate.browser_download_url === entry.url,
    );
    entry.url = asset.url;
  }

  const normalized = normalizeUpdaterManifest(fixture);

  for (const entry of Object.values(normalized.platforms)) {
    assert.match(
      entry.url,
      /^https:\/\/github\.com\/example\/mine-mail\/releases\/download\/v1\.2\.3\//,
    );
  }
  assert.equal(
    validateReleaseAssets({ ...fixture, manifest: normalized }),
    `Validated the ${tag} release asset and updater matrix.`,
  );
  assert.deepEqual(
    normalizeUpdaterManifest({ ...fixture, manifest: normalized }),
    normalized,
  );
});

test("rejects updater URLs outside the release during normalization", () => {
  const fixture = releaseFixture();
  fixture.manifest.platforms["windows-x86_64-nsis"].url =
    "https://downloads.example.invalid/Mine.Mail_1.2.3_x64-setup.exe";

  assert.throws(
    () => normalizeUpdaterManifest(fixture),
    /does not belong to this GitHub Release/,
  );
});

test("requires browser download URLs in the validated manifest", () => {
  const fixture = releaseFixture();
  const updaterAsset = fixture.releaseAssets.find(
    (asset) => asset.name === `Mine.Mail_${version}_x64-setup.exe`,
  );
  fixture.manifest.platforms["windows-x86_64-nsis"].url = updaterAsset.url;

  assert.throws(
    () => validateReleaseAssets(fixture),
    /must use the GitHub browser download URL/,
  );
});

test("macOS release build produces both installer and updater bundles", () => {
  const workflow = fs.readFileSync(
    new URL("../workflows/release.yml", import.meta.url),
    "utf8",
  );
  const macosMatrix = workflow.match(
    /- label: macOS Apple Silicon([\s\S]*?)(?=\n\s*- label:)/,
  )?.[1];

  assert.ok(macosMatrix, "macOS Apple Silicon release matrix is missing");
  assert.match(macosMatrix, /--bundles app,dmg/);
});

test("release publishing normalizes and uploads updater manifests", () => {
  for (const workflowName of ["release.yml", "publish-existing-release.yml"]) {
    const workflow = fs.readFileSync(
      new URL(`../workflows/${workflowName}`, import.meta.url),
      "utf8",
    );
    assert.match(workflow, /normalize-updater-manifest\.mjs/);
    assert.match(
      workflow,
      /gh release upload[\s\S]*latest\.json[\s\S]*--clobber/,
    );
  }
});

test("rejects an installer outside the supported release matrix", () => {
  const fixture = releaseFixture();
  fixture.releaseAssets.push({
    name: `Mine.Mail_${version}_x64_en-US.msi`,
    url: "https://api.github.com/repos/example/mine-mail/releases/assets/99",
    browser_download_url: `https://github.com/example/mine-mail/releases/download/${tag}/Mine.Mail_${version}_x64_en-US.msi`,
  });
  assert.throws(
    () => validateReleaseAssets(fixture),
    /contains unsupported assets: .*\.msi/,
  );
});

test("rejects an unsupported updater platform", () => {
  const fixture = releaseFixture();
  fixture.manifest.platforms["darwin-x86_64"] =
    fixture.manifest.platforms["darwin-aarch64"];
  assert.throws(
    () => validateReleaseAssets(fixture),
    /unsupported updater platforms: darwin-x86_64/,
  );
});

test("requires each package-specific updater to use the matching asset", () => {
  const fixture = releaseFixture();
  fixture.manifest.platforms["linux-x86_64-deb"].url =
    fixture.manifest.platforms["linux-x86_64-appimage"].url;
  assert.throws(
    () => validateReleaseAssets(fixture),
    /linux-x86_64-deb must reference .*\.deb, not .*\.AppImage/,
  );
});
