import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";

import { configureWindowsUpdater } from "./configure-windows-updater.mjs";
import { normalizeUpdaterManifest } from "./normalize-updater-manifest.mjs";
import { validateReleaseAssets } from "./validate-release-assets.mjs";

const tag = "v1.2.3";
const version = tag.slice(1);
const keyId = "0123456789abcdef";
const repository = "example/mine-mail";

function minisignEnvelope() {
  const payload = Buffer.alloc(10);
  Buffer.from(keyId, "hex").copy(payload, 2);
  return Buffer.from(
    `untrusted comment: test envelope\n${payload.toString("base64")}\n`,
  ).toString("base64");
}

function releaseFixture({ draft = false } = {}) {
  const names = [
    "latest.json",
    `Mine-Mail_${version}_windows-x64-installer.exe`,
    `Mine-Mail_${version}_windows-x64-updater.exe`,
    `Mine.Mail_${version}_aarch64.dmg`,
    `Mine.Mail_${version}_aarch64.app.tar.gz`,
    `Mine.Mail_${version}_amd64.deb`,
    `Mine.Mail_${version}_amd64.AppImage`,
  ];
  const releaseAssets = names.map((name, index) => ({
    name,
    url: `https://api.github.com/repos/example/mine-mail/releases/assets/${index + 1}`,
    browser_download_url: `https://github.com/${repository}/releases/download/${draft ? "untagged-a8cd3177c258479dab31" : tag}/${name}`,
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
    "windows-x86_64": `Mine-Mail_${version}_windows-x64-updater.exe`,
    "windows-x86_64-nsis": `Mine-Mail_${version}_windows-x64-updater.exe`,
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
    repository,
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

test("redirects generated Windows updater entries to the distinctly named updater", () => {
  const fixture = releaseFixture({ draft: true });
  const internalUpdaterName = `Mine.Mail_${version}_x64-setup.exe`;
  const internalUpdater = {
    name: internalUpdaterName,
    url: "https://api.github.com/repos/example/mine-mail/releases/assets/99",
    browser_download_url: `https://github.com/${repository}/releases/download/untagged-a8cd3177c258479dab31/${internalUpdaterName}`,
  };
  fixture.releaseAssets.push(internalUpdater);
  for (const platform of ["windows-x86_64", "windows-x86_64-nsis"]) {
    fixture.manifest.platforms[platform].url = internalUpdater.url;
  }

  const configured = configureWindowsUpdater(fixture);

  for (const platform of ["windows-x86_64", "windows-x86_64-nsis"]) {
    assert.equal(
      configured.platforms[platform].url,
      `https://github.com/${repository}/releases/download/${tag}/Mine-Mail_${version}_windows-x64-updater.exe`,
    );
    assert.equal(configured.platforms[platform].signature, minisignEnvelope());
  }
});

test("keeps already configured Windows updater metadata stable during draft recovery", () => {
  const fixture = releaseFixture();
  assert.deepEqual(configureWindowsUpdater(fixture), fixture.manifest);
});

test("rejects Windows updater metadata from outside the generated release assets", () => {
  const fixture = releaseFixture();
  fixture.manifest.platforms["windows-x86_64"].url =
    "https://downloads.example.invalid/internal.exe";

  assert.throws(
    () => configureWindowsUpdater(fixture),
    /must originate from the generated or dedicated Windows updater asset/,
  );
});

test("normalizes updater API asset URLs to version-pinned download URLs", () => {
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

test("replaces draft release URLs with version-pinned download URLs", () => {
  const fixture = releaseFixture({ draft: true });
  const normalized = normalizeUpdaterManifest(fixture);

  for (const entry of Object.values(normalized.platforms)) {
    assert.match(
      entry.url,
      /^https:\/\/github\.com\/example\/mine-mail\/releases\/download\/v1\.2\.3\//,
    );
    assert.doesNotMatch(entry.url, /\/untagged-/);
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
    "https://downloads.example.invalid/Mine-Mail_1.2.3_windows-x64-updater.exe";

  assert.throws(
    () => normalizeUpdaterManifest(fixture),
    /does not belong to this GitHub Release/,
  );
});

test("requires version-pinned download URLs in the validated manifest", () => {
  const fixture = releaseFixture();
  const updaterAsset = fixture.releaseAssets.find(
    (asset) =>
      asset.name === `Mine-Mail_${version}_windows-x64-updater.exe`,
  );
  fixture.manifest.platforms["windows-x86_64-nsis"].url = updaterAsset.url;

  assert.throws(
    () => validateReleaseAssets(fixture),
    /must use the version-pinned GitHub download URL/,
  );
});

test("rejects draft release URLs in the validated manifest", () => {
  const fixture = releaseFixture({ draft: true });

  assert.throws(
    () => validateReleaseAssets(fixture),
    /must use the version-pinned GitHub download URL/,
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
    assert.match(workflow, /configure-windows-updater\.mjs/);
    assert.match(workflow, /gh release delete-asset/);
    assert.match(
      workflow,
      /gh release upload[\s\S]*latest\.json[\s\S]*--clobber/,
    );
  }
});

test("Windows release publishes distinctly named installer and updater packages", () => {
  const workflow = fs.readFileSync(
    new URL("../workflows/release.yml", import.meta.url),
    "utf8",
  );
  const buildScript = fs.readFileSync(
    new URL("../../installer/windows/scripts/build-release.ps1", import.meta.url),
    "utf8",
  );

  assert.match(
    workflow,
    /Mine-Mail_\$\{version\}_windows-x64-updater\.exe/,
  );
  assert.match(
    buildScript,
    /Mine-Mail_\$\{Version\}_windows-x64-installer\.exe/,
  );
  assert.match(workflow, /Mine\.Mail_\$\{version\}_x64-setup\.exe/);
  assert.doesNotMatch(workflow, /signer sign/);
  assert.match(workflow, /Windows 11 x64（AMD\/Intel 64 位，推荐首次安装）/);
  assert.match(workflow, /Windows 11 x64（应用内自动更新专用）/);
  assert.match(workflow, /macOS 14\+ Apple Silicon（应用内自动更新专用）/);
});

test("release skill writes and verifies platform-specific GitHub Release notes", () => {
  const skill = fs.readFileSync(
    new URL("../../.agents/skills/release-mine-mail/SKILL.md", import.meta.url),
    "utf8",
  );

  assert.match(skill, /## 下载与系统要求/);
  assert.match(skill, /Mine-Mail_X\.Y\.Z_windows-x64-installer\.exe/);
  assert.match(skill, /Mine-Mail_X\.Y\.Z_windows-x64-updater\.exe/);
  assert.match(skill, /Windows 11 x64（AMD\/Intel 64 位，推荐首次安装）/);
  assert.match(skill, /gh release edit vX\.Y\.Z/);
  assert.match(skill, /Read the Release body back/);
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

test("rejects the internal Windows updater in a public release", () => {
  const fixture = releaseFixture();
  fixture.releaseAssets.push({
    name: `Mine.Mail_${version}_x64-setup.exe`,
    url: "https://api.github.com/repos/example/mine-mail/releases/assets/99",
    browser_download_url: `https://github.com/example/mine-mail/releases/download/${tag}/Mine.Mail_${version}_x64-setup.exe`,
  });
  assert.throws(
    () => validateReleaseAssets(fixture),
    /contains unsupported assets: Mine\.Mail_.*_x64-setup\.exe/,
  );
});

test("requires Windows update metadata to reference the updater package", () => {
  const fixture = releaseFixture();
  const installer = fixture.releaseAssets.find(
    (asset) =>
      asset.name === `Mine-Mail_${version}_windows-x64-installer.exe`,
  );
  fixture.manifest.platforms["windows-x86_64"].url =
    installer.browser_download_url;

  assert.throws(
    () => validateReleaseAssets(fixture),
    /windows-x86_64 must reference .*_windows-x64-updater\.exe/,
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
