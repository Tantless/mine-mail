#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const VERSION_PATTERN = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

function fail(message) {
  console.error(`set-version: ${message}`);
  process.exit(1);
}

const rawArgs = process.argv.slice(2);
const checkOnly = rawArgs.includes("--check");
const positional = rawArgs.filter((argument) => argument !== "--check");

if (positional.length !== 1) {
  fail("usage: node set-version.mjs vX.Y.Z [--check]");
}

const version = positional[0].replace(/^v/, "");
const tag = `v${version}`;

if (!VERSION_PATTERN.test(version)) {
  fail(`"${positional[0]}" must match vX.Y.Z or vX.Y.Z-prerelease`);
}

let repoRoot;
try {
  repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  }).trim();
} catch {
  fail("run this script inside the Mine Mail Git repository");
}

const files = new Map();

function read(relativePath) {
  if (!files.has(relativePath)) {
    const absolutePath = path.join(repoRoot, relativePath);
    if (!fs.existsSync(absolutePath)) {
      fail(`missing required file ${relativePath}`);
    }
    files.set(relativePath, fs.readFileSync(absolutePath, "utf8"));
  }
  return files.get(relativePath);
}

function cargoPackagePattern(packageName) {
  return new RegExp(
    `(\\[\\[package\\]\\]\\r?\\nname = "${packageName}"\\r?\\nversion = ")([^"]+)(")`,
  );
}

const fields = [
  {
    label: "root Cargo package",
    file: "Cargo.toml",
    pattern: /(\[package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m,
  },
  {
    label: "root Cargo lock package",
    file: "Cargo.lock",
    pattern: cargoPackagePattern("mine-mail"),
  },
  {
    label: "desktop Cargo package",
    file: "web/src-tauri/Cargo.toml",
    pattern: /(\[package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m,
  },
  {
    label: "core package in desktop Cargo lock",
    file: "web/src-tauri/Cargo.lock",
    pattern: cargoPackagePattern("mine-mail"),
  },
  {
    label: "desktop package in desktop Cargo lock",
    file: "web/src-tauri/Cargo.lock",
    pattern: cargoPackagePattern("mine-mail-desktop"),
  },
  {
    label: "Tauri application version",
    file: "web/src-tauri/tauri.conf.json",
    pattern: /("version"\s*:\s*")([^"]+)(")/,
  },
  {
    label: "installer preview version",
    file: "installer/windows/src/installerState.js",
    pattern:
      /(export function defaultPreviewInfo\(\)[\s\S]*?version:\s*")([^"]+)(")/,
  },
  {
    label: "installer preview test version",
    file: "installer/windows/src/installerState.test.js",
    pattern:
      /(expect\(defaultPreviewInfo\(\)\)\.toMatchObject\(\{[\s\S]*?version:\s*")([^"]+)(")/,
  },
];

const currentVersions = fields.map((field) => {
  const match = read(field.file).match(field.pattern);
  if (!match) {
    fail(`could not locate ${field.label} in ${field.file}`);
  }
  if (!VERSION_PATTERN.test(match[2])) {
    fail(`${field.label} has invalid version "${match[2]}"`);
  }
  return { ...field, version: match[2] };
});

const uniqueCurrentVersions = [
  ...new Set(currentVersions.map((field) => field.version)),
];

if (uniqueCurrentVersions.length !== 1) {
  const details = currentVersions
    .map((field) => `${field.label}=${field.version}`)
    .join(", ");
  fail(`canonical versions are inconsistent: ${details}`);
}

const currentVersion = uniqueCurrentVersions[0];

function compareIdentifiers(left, right) {
  const leftNumeric = /^\d+$/.test(left);
  const rightNumeric = /^\d+$/.test(right);
  if (leftNumeric && rightNumeric) return Number(left) - Number(right);
  if (leftNumeric) return -1;
  if (rightNumeric) return 1;
  return left.localeCompare(right);
}

function compareVersions(left, right) {
  const splitVersion = (value) => {
    const prereleaseSeparator = value.indexOf("-");
    if (prereleaseSeparator === -1) return [value, undefined];
    return [
      value.slice(0, prereleaseSeparator),
      value.slice(prereleaseSeparator + 1),
    ];
  };
  const [leftCore, leftPre] = splitVersion(left);
  const [rightCore, rightPre] = splitVersion(right);
  const leftNumbers = leftCore.split(".").map(Number);
  const rightNumbers = rightCore.split(".").map(Number);

  for (let index = 0; index < 3; index += 1) {
    if (leftNumbers[index] !== rightNumbers[index]) {
      return leftNumbers[index] - rightNumbers[index];
    }
  }

  if (leftPre === undefined && rightPre === undefined) return 0;
  if (leftPre === undefined) return 1;
  if (rightPre === undefined) return -1;

  const leftParts = leftPre.split(".");
  const rightParts = rightPre.split(".");
  const length = Math.max(leftParts.length, rightParts.length);

  for (let index = 0; index < length; index += 1) {
    if (leftParts[index] === undefined) return -1;
    if (rightParts[index] === undefined) return 1;
    const compared = compareIdentifiers(leftParts[index], rightParts[index]);
    if (compared !== 0) return compared;
  }
  return 0;
}

if (checkOnly) {
  if (currentVersion !== version) {
    fail(`canonical version is ${currentVersion}, expected ${version}`);
  }
  console.log(`All canonical Mine Mail versions match ${tag}.`);
  process.exit(0);
}

if (currentVersion === version) {
  console.log(`All canonical Mine Mail versions already match ${tag}.`);
  process.exit(0);
}

if (compareVersions(version, currentVersion) <= 0) {
  fail(`target ${tag} must be greater than current v${currentVersion}`);
}

for (const field of fields) {
  const source = read(field.file);
  const updated = source.replace(
    field.pattern,
    (_match, prefix, _oldVersion, suffix) => `${prefix}${version}${suffix}`,
  );
  if (updated === source) {
    fail(`failed to update ${field.label} in ${field.file}`);
  }
  files.set(field.file, updated);
}

for (const [relativePath, contents] of files) {
  fs.writeFileSync(path.join(repoRoot, relativePath), contents, "utf8");
}

for (const field of fields) {
  const match = fs
    .readFileSync(path.join(repoRoot, field.file), "utf8")
    .match(field.pattern);
  if (!match || match[2] !== version) {
    fail(`post-write validation failed for ${field.label} in ${field.file}`);
  }
}

console.log(
  `Updated ${files.size} Mine Mail release files from v${currentVersion} to ${tag}.`,
);
