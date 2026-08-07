# Mine Mail Release Gate

This is the living checklist for the next public release. It contains open
release decisions and gates only. Completed investigations and historical QA
belong in Git history.

Verify every item against the release commit, provider console, signing account,
CI configuration, and a clean target device before checking it.

## Release scope and ownership

- [ ] Choose the channel: invited beta, public beta, or stable.
- [ ] Declare supported operating systems, architectures, providers, and known
  limits consistently in the app, README, installer, and release notes.
- [ ] Confirm the publisher, OAuth brand, website, signing identity, support
  contact, and privacy/data-deletion pages identify the same release owner.
- [ ] Assign owners for signing keys, OAuth review, security reports, release
  approval, rollback, and user support.

## Provider, privacy, and security

- [ ] Separate development/test OAuth configuration from the production project
  and complete any required provider verification.
- [ ] Test account login, refresh, revocation, password changes, expired
  credentials, reauthentication, and removal for every supported provider.
- [ ] Review published privacy, terms, support, and data-deletion pages against
  the shipped defaults and data flow.
- [ ] Document local persistent data, retention/deletion controls, and the
  decision that the SQLite mail cache is not whole-database encrypted.
- [ ] Verify OS credential storage and local file permissions on every supported
  platform.
- [ ] Security-review MIME parsing, sanitizer/iframe/CSP boundaries, remote-image
  policy, safe links, TLS, draft conflicts, managed attachments, Outbox
  idempotency, and uncertain delivery.
- [ ] Scan representative logs and final artifacts for credentials, tokens,
  addresses, subjects, bodies, RFC822 source, private configuration, and complete
  local paths.
- [ ] Review dependency advisories and licenses; produce third-party notices and
  an SBOM or explicitly accept the remaining risk.

## Build, signing, and updates

- [ ] Pin and review release workflow dependencies and apply least-privilege
  permissions.
- [ ] Protect production signing and publishing with an independently approved
  environment.
- [ ] Make version consistency, tests, build/check, signing, signature
  verification, malware scanning, hashes, updater metadata, and draft-release
  creation one fail-closed pipeline.
- [ ] Sign and timestamp every shipped executable, installer, uninstaller, update
  payload, and bootstrap component with the durable publisher identity.
- [ ] Keep beta and stable update channels separate.
- [ ] Verify updater manifests use version-pinned public artifacts and point to
  the dedicated Windows updater payload, not the recommended first-install
  package.
- [ ] Test signed update success plus interruption, offline, invalid signature,
  disk-full, migration, skipped-version, rollback, and emergency withdrawal.
- [ ] Publish expected publisher identities and artifact SHA-256 values.

## Native-platform acceptance

- [ ] Windows: validate the declared minimum version and architecture, install,
  custom location, upgrade, uninstall, autostart, shortcuts, updater, non-admin
  use, Chinese paths/usernames, insufficient disk, and recovery.
- [ ] macOS: complete Developer ID signing, notarization, stapling, Apple Silicon
  testing on the declared minimum version, Keychain, tray, notification,
  autostart, sleep/wake, updater, and uninstall checks.
- [ ] Linux: validate DEB and AppImage on the declared x64 distributions,
  including package signing, secret storage, WebKitGTK, tray, notification,
  autostart, sleep/wake, updater, and uninstall behavior.
- [ ] Publish only platforms that passed their complete signing and native-device
  gate; keep other builds internal.

### Attachment Save As matrix

- [ ] Exercise Save As on the supported local and SMB filesystems for each
  platform. Verify collision suffixes, concurrent same-name saves, exact decoded
  bytes, and that existing files never change.
- [ ] Exercise permission loss, disk-full, removed media, cancellation, and
  interruption. Confirm no partial final name remains.
- [ ] Review the residual path-based race in
  `tempfile::NamedTempFile::persist_noclobber`. Mine Mail rejects a selected
  directory whose final component is a symlink or Windows reparse point, but it
  does not yet anchor staging and publication to an opened directory handle.
  Block release if the chosen threat model requires protection from hostile
  same-user directory replacement.

## Release-candidate acceptance

- [ ] Run all applicable checks from `../AGENTS.md` on the exact release commit.
- [ ] Test clean install, first launch, every supported account type, initial
  synchronization, close-to-tray, and reopen.
- [ ] Test three-account startup/manual/tray/scheduled sync, switching, credential
  failure, notification baselines, and account removal.
- [ ] Test online/offline startup, network loss/recovery, DNS/TLS failure,
  sleep/wake, and system-time changes.
- [ ] Test list history, local search, all mail render modes, remote images,
  reply/forward, attachments, stars, mailbox actions, contacts, remarks, and
  avatars.
- [ ] Test new/existing drafts, periodic remote draft sync, version conflicts,
  read-only MIME, recipient binding, confirmed send failure, uncertain delivery,
  and explicit retry.
- [ ] Test keyboard navigation, visible focus, reduced motion, high DPI, zoom,
  long identity/content, Chinese text, and narrow desktop reflows in every theme.
- [ ] Re-download public artifacts, verify hashes/signatures, install, update, and
  confirm release notes list features, migrations, rollback guidance, and known
  limitations accurately.
