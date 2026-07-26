# Mine Mail Release Gate

This is a living checklist for the next public release. It contains only open
release decisions and gates; completed research and historical QA belong in Git
history, not here.

Before checking an item, verify it against the current code, CI configuration,
provider console, signing account, and a clean target device.

## Release definition

- [ ] Choose the release channel: invited beta, public beta, or stable.
- [ ] Declare the supported OS versions, CPU architectures, providers, and known
  feature limits consistently in the app, website, installer, and release notes.
- [ ] Choose one primary consumer installer per supported platform.
- [ ] Confirm the public publisher name, OAuth brand, website identity, signing
  identity, and support contact all refer to the same release owner.
- [ ] Assign owners for signing keys, OAuth review, security reports, release
  approval, and user support.

## Windows distribution

- [ ] Select and provision a publicly trusted Windows code-signing service or
  certificate for the durable publisher identity.
- [ ] Sign and timestamp every shipped executable, installer, uninstaller, update
  payload, and bootstrap component.
- [ ] Make the release job fail closed when signing or signature verification is
  unavailable.
- [ ] Verify install, upgrade, uninstall, autostart, shortcut behavior, updater,
  non-admin use, Chinese paths/usernames, insufficient disk, and recovery on a
  clean supported Windows device.
- [ ] Publish the expected publisher identity and artifact SHA-256 values.

## Gmail and provider readiness

- [ ] Separate development/test OAuth configuration from the production project.
- [ ] Confirm production redirect behavior, scopes, application identity, and
  support/privacy URLs match the shipped desktop flow.
- [ ] Complete any required provider brand or restricted-scope verification
  before allowing public Gmail sign-in.
- [ ] Test login, refresh, revocation, password/account changes, consent removal,
  expired credentials, and reauthentication without exposing tokens in logs or
  artifacts.
- [ ] Verify account removal clearly distinguishes disconnect, local-cache
  deletion, and provider-token revocation.

## Privacy, security, and legal

- [ ] Add a repository-level `LICENSE` matching the Cargo package declarations.
- [ ] Add `SECURITY.md` with a private vulnerability-reporting path and supported
  versions.
- [ ] Review the public privacy, terms, support, and data-deletion pages against
  the shipped data flow and defaults.
- [ ] Document local persistent data, paths, retention/deletion controls, and the
  decision that the SQLite mail cache is not whole-database encrypted.
- [ ] Verify OS credential-store use and local file permissions on every supported
  platform.
- [ ] Security-review MIME parsing, sanitizer/iframe/CSP boundaries, remote-image
  policy, safe link schemes, TLS verification, draft conflicts, Outbox
  idempotency, and uncertain delivery.
- [ ] Scan representative logs and final artifacts for credentials, tokens,
  addresses, subjects, body content, RFC822 source, private configuration, and
  complete local paths.
- [ ] Review dependency advisories and licenses; produce third-party notices and
  an SBOM or explicitly accept the remaining risk.

## CI, updater, and release control

- [ ] Pin and review release workflow dependencies and apply least-privilege
  permissions.
- [ ] Gate production signing and publishing behind a protected environment and
  independent human approval.
- [ ] Make version consistency, tests, build/check, signing, signature
  verification, malware scanning, hashes, updater metadata, and draft release
  creation one fail-closed pipeline.
- [ ] Keep beta and stable update channels separate.
- [ ] Test signed update success plus interruption, offline, invalid signature,
  disk-full, migration, skipped-version, rollback, and emergency withdrawal
  behavior.
- [ ] Publish only platforms that passed their complete signing and native-device
  gate; keep other builds as internal artifacts.

## Native-platform acceptance

- [ ] Validate Windows on the declared minimum version and architecture.
- [ ] Before publishing macOS, complete Developer ID signing, notarization,
  stapling, Intel/Apple Silicon testing, Keychain, tray, notification, autostart,
  sleep/wake, updater, and uninstall checks.
- [ ] Before publishing Linux, choose supported distributions/package formats and
  validate package signing, secret storage, WebKitGTK, tray, notification,
  autostart, sleep/wake, updater, and uninstall behavior.

## Release-candidate acceptance

- [ ] Run root Rust, React, and Tauri verification from `../AGENTS.md` on the exact
  release commit.
- [ ] Test clean install → first launch → add each supported provider → first sync
  → close to tray → reopen.
- [ ] Test three-account startup/manual/tray/scheduled sync, switching,
  credential failure, notification baselines, and account deletion.
- [ ] Test online/offline startup, network loss/recovery, DNS/TLS failure,
  sleep/wake, and system-time changes.
- [ ] Test summary/body hydration, search, all three mail render modes, reply
  history, remote-image policies, stars, contacts, remarks, and avatars.
- [ ] Test new/existing draft close semantics, five-minute remote draft sync,
  version conflicts, read-only MIME, recipient confirmation, confirmed failure,
  `delivery_unknown`, and manual retry.
- [ ] Test keyboard navigation, visible focus, reduced motion, high DPI, zoom,
  long names/addresses/subjects, Chinese text, and narrow desktop windows in every
  theme.
- [ ] Re-download public artifacts, verify hashes/signatures, install, update, and
  confirm release notes accurately list features, migrations, rollback guidance,
  and known limitations.
