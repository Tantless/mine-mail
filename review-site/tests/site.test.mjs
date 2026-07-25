import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");
const dist = resolve(root, "dist");

const pages = [
  ["home", "index.html", "https://minemail.tantless.online/"],
  ["privacy", "privacy/index.html", "https://minemail.tantless.online/privacy/"],
  ["terms", "terms/index.html", "https://minemail.tantless.online/terms/"],
  ["support", "support/index.html", "https://minemail.tantless.online/support/"],
  [
    "data deletion",
    "data-deletion/index.html",
    "https://minemail.tantless.online/data-deletion/",
  ],
];

test("build emits every public review page with canonical metadata", async () => {
  for (const [name, path, canonical] of pages) {
    const fullPath = resolve(dist, path);
    assert.equal(existsSync(fullPath), true, `${name} page is missing`);
    const html = await readFile(fullPath, "utf8");
    assert.match(html, new RegExp(`rel="canonical" href="${canonical}"`));
    assert.match(html, /tantless8@gmail\.com/);
    assert.match(html, /冀ICP备2026010199号/);
  }
});

test("privacy policy contains Google restricted-scope and Limited Use disclosures", async () => {
  const html = await readFile(resolve(dist, "privacy/index.html"), "utf8");
  assert.match(html, /https:\/\/mail\.google\.com\//);
  assert.match(html, /Google API Services User Data Policy/);
  assert.match(html, /Limited Use requirements/);
  assert.match(html, /The app distinguishes each deletion scope/);
  assert.match(html, /Revoke authorization and remove/);
  assert.match(html, /SQLite database is not encrypted as a whole/);
});

test("public pages do not embed localhost, trackers, or a download promise", async () => {
  for (const [, path] of pages) {
    const html = await readFile(resolve(dist, path), "utf8");
    assert.doesNotMatch(html, /localhost|127\.0\.0\.1/);
    assert.doesNotMatch(html, /google-analytics|googletagmanager|facebook\.net/i);
  }
  const home = await readFile(resolve(dist, "index.html"), "utf8");
  assert.match(home, /Public beta in preparation/);
  assert.match(home, /downloads are not yet open/);
});

test("homepage exposes an exact app identity and purpose without JavaScript", async () => {
  const home = await readFile(resolve(dist, "index.html"), "utf8");
  assert.match(home, /<h1>Mine Mail<\/h1>/);
  assert.match(home, />本地优先桌面邮件客户端</);
  assert.match(home, />Local-first desktop email client</);
  assert.match(home, /"@type": "SoftwareApplication"/);
  assert.match(home, /"name": "Mine Mail"/);
});

test("static discovery and brand assets are included", () => {
  for (const path of [
    "robots.txt",
    "sitemap.xml",
    "site.webmanifest",
    "brand/mine-mail-fox.png",
    "brand/mine-mail-social.png",
    "brand/wallpaper-dusk.png",
  ]) {
    assert.equal(existsSync(resolve(dist, path)), true, `${path} is missing`);
  }
});
