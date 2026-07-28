use std::{
    collections::HashSet,
    fs::{self, File, Metadata},
    io::{self, Cursor, Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    MailError, Result,
    atomic_publish::{AtomicNoClobberFile, PublishAttempt},
    mime::{MAX_MANAGED_ATTACHMENT_BYTES, attachment_name_candidate, safe_attachment_filename},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImportedManagedAttachment {
    pub id: String,
    /// One validated base name relative to the managed blob directory.
    pub internal_name: String,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    /// Lowercase SHA-256 of the immutable blob bytes. SQLite persists this
    /// digest so an equal-length local replacement cannot enter MIME output.
    pub sha256_hex: String,
}

/// Rust-owned immutable attachment storage. No method returns a complete path,
/// and callers can address a blob only by the validated internal base name
/// persisted in SQLite.
#[derive(Clone, Debug)]
pub(crate) struct ManagedAttachmentStore {
    managed_root: PathBuf,
    blob_root: PathBuf,
}

impl ManagedAttachmentStore {
    pub(crate) fn new(
        product_data_root: impl AsRef<Path>,
        stable_account_id: &str,
    ) -> Result<Self> {
        let product_data_root = product_data_root.as_ref();
        if !product_data_root.is_absolute() {
            return Err(MailError::Validation(
                "managed attachment storage requires an absolute product-data root".to_owned(),
            ));
        }
        fs::create_dir_all(product_data_root)?;
        let product_data_root = fs::canonicalize(product_data_root)?;
        let managed_root = product_data_root.join("managed-attachments");
        fs::create_dir_all(&managed_root)?;
        validate_managed_directory(&managed_root)?;
        let managed_root = fs::canonicalize(managed_root)?;
        if !managed_root.starts_with(&product_data_root) {
            return Err(MailError::Validation(
                "managed attachment storage escaped the product-data root".to_owned(),
            ));
        }
        let account_scope = account_scope_component(stable_account_id)?;
        let expected_blob_root = managed_root.join(account_scope);
        fs::create_dir_all(&expected_blob_root)?;
        validate_managed_directory(&expected_blob_root)?;
        let blob_root = fs::canonicalize(&expected_blob_root)?;
        validate_account_storage_target(&blob_root, &expected_blob_root, &managed_root)?;
        Ok(Self {
            managed_root,
            blob_root,
        })
    }

    /// Imports a platform-picker result by copying its complete bytes. The
    /// original path is never retained and never appears in the returned value.
    pub(crate) fn import_file(
        &self,
        selected_source: impl AsRef<Path>,
    ) -> Result<ImportedManagedAttachment> {
        let selected_source = selected_source.as_ref();
        let before = fs::metadata(selected_source)?;
        if !before.is_file() {
            return Err(MailError::Validation(
                "the selected attachment is not a regular file".to_owned(),
            ));
        }
        if before.len() > MAX_MANAGED_ATTACHMENT_BYTES {
            return Err(MailError::Validation(
                "the selected attachment exceeds the managed attachment size limit".to_owned(),
            ));
        }
        let original_name = selected_source
            .file_name()
            .map(|value| value.to_string_lossy().into_owned());
        let name = safe_attachment_filename(original_name.as_deref());
        let mime_type = mime_type_for_name(&name).to_owned();
        let source = File::open(selected_source)?;
        let imported = self.import_reader(source, &name, &mime_type, before.len())?;
        let after = fs::metadata(selected_source)?;
        let modification_changed = before
            .modified()
            .ok()
            .zip(after.modified().ok())
            .is_some_and(|(before, after)| before != after);
        if !after.is_file() || after.len() != before.len() || modification_changed {
            let _ = self.remove_internal_file(&imported.internal_name);
            return Err(MailError::Validation(
                "the selected attachment changed while it was imported".to_owned(),
            ));
        }
        Ok(imported)
    }

    /// Stages one decoded received-message part into the immutable managed
    /// area. The opaque part identity is persisted separately by the
    /// repository and is never reused as a file-system name.
    pub(crate) fn import_bytes(
        &self,
        bytes: &[u8],
        proposed_name: &str,
        mime_type: &str,
    ) -> Result<ImportedManagedAttachment> {
        if bytes.len() as u64 > MAX_MANAGED_ATTACHMENT_BYTES {
            return Err(MailError::Validation(
                "the decoded attachment exceeds the managed attachment size limit".to_owned(),
            ));
        }
        self.import_reader(
            Cursor::new(bytes),
            proposed_name,
            mime_type,
            bytes.len() as u64,
        )
    }

    fn import_reader(
        &self,
        source: impl Read,
        proposed_name: &str,
        mime_type: &str,
        expected_size: u64,
    ) -> Result<ImportedManagedAttachment> {
        self.validate_blob_root()?;
        if expected_size > MAX_MANAGED_ATTACHMENT_BYTES {
            return Err(MailError::Validation(
                "the selected attachment exceeds the managed attachment size limit".to_owned(),
            ));
        }
        let name = safe_attachment_filename(Some(proposed_name));
        let mime_type = normalize_mime_type(mime_type);
        let mut publication = AtomicNoClobberFile::new_managed_attachment_in(&self.blob_root)?;
        // Read one byte beyond the metadata snapshot so a growing or
        // adversarial source is detected without allowing an unbounded
        // temporary file. `expected_size` was bounded above, so this is at
        // most the product limit plus one byte.
        let read_limit = expected_size.checked_add(1).ok_or_else(|| {
            MailError::Validation("the selected attachment size limit overflowed".to_owned())
        })?;
        let mut source = source.take(read_limit);
        let mut hasher = Sha256::new();
        let mut copied = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        publication.stage(|temporary_file| -> Result<()> {
            loop {
                let read = source.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                temporary_file.write_all(&buffer[..read])?;
                hasher.update(&buffer[..read]);
                copied = copied.checked_add(read as u64).ok_or_else(|| {
                    MailError::Validation("the selected attachment size overflowed".to_owned())
                })?;
            }
            if copied != expected_size {
                return Err(MailError::Validation(
                    "the selected attachment size changed during import".to_owned(),
                ));
            }
            Ok(())
        })?;

        let (id, internal_name) =
            publish_managed_blob(&mut publication, || Uuid::now_v7().to_string())?;

        Ok(ImportedManagedAttachment {
            id,
            internal_name,
            name,
            mime_type,
            size_bytes: copied,
            sha256_hex: lowercase_hex(&hasher.finalize()),
        })
    }

    pub(crate) fn open_internal_file(&self, internal_name: &str) -> Result<File> {
        let path = self.internal_path(internal_name)?;
        let before = validate_managed_regular_file(&path)?;
        let file = File::open(&path)?;
        let opened = file.metadata()?;
        if !opened.is_file() || opened.len() != before.len() {
            return Err(MailError::Validation(
                "a managed attachment changed while it was opened".to_owned(),
            ));
        }
        // Revalidate the path after opening. If a local actor exchanged it
        // during the check/open window, the path is rejected; an exchange
        // after this point cannot redirect the already-open file handle.
        let after = validate_managed_regular_file(&path)?;
        if after.len() != opened.len()
            || before
                .modified()
                .ok()
                .zip(after.modified().ok())
                .is_some_and(|(before, after)| before != after)
        {
            return Err(MailError::Validation(
                "a managed attachment changed while it was opened".to_owned(),
            ));
        }
        Ok(file)
    }

    /// Reads one immutable blob after checking its persisted byte count and
    /// digest. MIME construction owns the returned bytes; no path or byte
    /// buffer crosses the desktop boundary.
    pub(crate) fn read_internal_file(
        &self,
        internal_name: &str,
        expected_size: u64,
        expected_sha256_hex: &str,
    ) -> Result<Vec<u8>> {
        validate_sha256_hex(expected_sha256_hex)?;
        let (bytes, actual_sha256_hex) =
            self.read_internal_file_for_digest_backfill(internal_name, expected_size)?;
        if actual_sha256_hex != expected_sha256_hex {
            return Err(MailError::Validation(
                "a managed attachment failed its immutable content check".to_owned(),
            ));
        }
        Ok(bytes)
    }

    /// Reads and hashes a legacy blob whose SQLite row has no persisted digest
    /// yet. The caller must atomically bind the returned digest to the exact
    /// account-scoped database record before using the bytes. This method is
    /// deliberately separate from ordinary reads so production MIME paths
    /// cannot accidentally fall back to length-only validation.
    pub(crate) fn read_internal_file_for_digest_backfill(
        &self,
        internal_name: &str,
        expected_size: u64,
    ) -> Result<(Vec<u8>, String)> {
        if expected_size > MAX_MANAGED_ATTACHMENT_BYTES {
            return Err(MailError::Validation(
                "a managed attachment exceeds the configured byte limit".to_owned(),
            ));
        }
        let mut file = self.open_internal_file(internal_name)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != expected_size {
            return Err(MailError::Validation(
                "a managed attachment no longer matches its immutable metadata".to_owned(),
            ));
        }
        let bytes = read_bounded_exact(&mut file, expected_size)?;
        let sha256_hex = lowercase_hex(&Sha256::digest(&bytes));
        Ok((bytes, sha256_hex))
    }

    pub(crate) fn remove_internal_file(&self, internal_name: &str) -> Result<bool> {
        let path = self.internal_path(internal_name)?;
        match validate_managed_regular_file(&path) {
            Ok(_) => {}
            Err(MailError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(false);
            }
            Err(error) => return Err(error),
        }
        match fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    /// Removes only interrupted import siblings. Final `.blob` files are left
    /// for repository reference accounting and orphan cleanup.
    pub(crate) fn cleanup_temporary_files(&self, minimum_age: Duration) -> Result<usize> {
        self.cleanup_temporary_files_at(minimum_age, SystemTime::now())
    }

    fn cleanup_temporary_files_at(
        &self,
        minimum_age: Duration,
        reference_time: SystemTime,
    ) -> Result<usize> {
        self.validate_blob_root()?;
        let mut removed = 0;
        for entry in fs::read_dir(&self.blob_root)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with(".tmp-") || !entry.file_type()?.is_file() {
                continue;
            }
            let old_enough = entry
                .metadata()?
                .modified()
                .ok()
                .and_then(|modified| reference_time.duration_since(modified).ok())
                .is_some_and(|age| age >= minimum_age);
            if !old_enough {
                continue;
            }
            match fs::remove_file(entry.path()) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(removed)
    }

    pub(crate) fn list_internal_names(&self) -> Result<Vec<String>> {
        self.validate_blob_root()?;
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.blob_root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if validate_internal_name(&name).is_ok() {
                validate_managed_regular_file(&entry.path())?;
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }

    /// Cleans final blobs that were staged by a process that stopped before
    /// registering them in SQLite. A minimum age protects a concurrent import
    /// that has not reached its atomic repository transaction yet.
    pub(crate) fn cleanup_unregistered_files(
        &self,
        registered_internal_names: &HashSet<String>,
        minimum_age: Duration,
    ) -> Result<usize> {
        let mut removed = 0;
        for internal_name in self.list_internal_names()? {
            if registered_internal_names.contains(&internal_name) {
                continue;
            }
            let path = self.internal_path(&internal_name)?;
            let old_enough = validate_managed_regular_file(&path)?
                .modified()
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age >= minimum_age);
            if !old_enough {
                continue;
            }
            if self.remove_internal_file(&internal_name)? {
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn internal_path(&self, internal_name: &str) -> Result<PathBuf> {
        validate_internal_name(internal_name)?;
        self.validate_blob_root()?;
        Ok(self.blob_root.join(internal_name))
    }

    fn validate_blob_root(&self) -> Result<()> {
        validate_managed_directory(&self.blob_root)?;
        let resolved = fs::canonicalize(&self.blob_root)?;
        validate_account_storage_target(&resolved, &self.blob_root, &self.managed_root)
    }

    /// Deletes only this stable account's managed directory. The caller owns
    /// the higher-level account-removal confirmation; no path is accepted from
    /// React or from an untrusted request.
    pub(crate) fn delete_account_storage(&self) -> Result<bool> {
        let resolved = match fs::canonicalize(&self.blob_root) {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        validate_managed_directory(&self.blob_root)?;
        validate_account_storage_target(&resolved, &self.blob_root, &self.managed_root)?;
        fs::remove_dir_all(&self.blob_root)?;
        Ok(true)
    }
}

fn read_bounded_exact(source: impl Read, expected_size: u64) -> Result<Vec<u8>> {
    if expected_size > MAX_MANAGED_ATTACHMENT_BYTES {
        return Err(MailError::Validation(
            "a managed attachment exceeds the configured byte limit".to_owned(),
        ));
    }
    let read_limit = expected_size.checked_add(1).ok_or_else(|| {
        MailError::Validation("a managed attachment size limit overflowed".to_owned())
    })?;
    let capacity = usize::try_from(read_limit).map_err(|_| {
        MailError::Validation("a managed attachment is too large to read safely".to_owned())
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    source.take(read_limit).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != expected_size {
        return Err(MailError::Validation(
            "a managed attachment changed while it was read".to_owned(),
        ));
    }
    Ok(bytes)
}

fn validate_account_storage_target(
    resolved: &Path,
    expected_blob_root: &Path,
    managed_root: &Path,
) -> Result<()> {
    if resolved != expected_blob_root || resolved.parent() != Some(managed_root) {
        return Err(MailError::Validation(
            "managed attachment account storage does not match its isolated root".to_owned(),
        ));
    }
    Ok(())
}

fn validate_managed_directory(path: &Path) -> Result<Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata_is_reparse(&metadata) {
        return Err(MailError::Validation(
            "managed attachment storage must be a real local directory".to_owned(),
        ));
    }
    Ok(metadata)
}

fn validate_managed_regular_file(path: &Path) -> Result<Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata_is_reparse(&metadata) {
        return Err(MailError::Validation(
            "a managed attachment must be a real regular file".to_owned(),
        ));
    }
    if fs::canonicalize(path)? != path {
        return Err(MailError::Validation(
            "a managed attachment escaped its expected file identity".to_owned(),
        ));
    }
    Ok(metadata)
}

#[cfg(windows)]
fn metadata_is_reparse(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn validate_sha256_hex(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(MailError::Validation(
            "a managed attachment content digest is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn lowercase_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// Writes a decoded received attachment beside the platform-selected Save As
/// destination. The selected base name is sanitized again, collisions receive
/// a numeric suffix, and publication never replaces an existing file.
pub(crate) fn save_extracted_file(
    selected_destination: &Path,
    default_safe_name: &str,
    bytes: &[u8],
) -> io::Result<String> {
    if !selected_destination.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the attachment destination must be absolute",
        ));
    }
    let directory = selected_destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the attachment destination has no parent directory",
        )
    })?;
    if !directory.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the attachment destination directory is unavailable",
        ));
    }

    let selected_name = selected_destination
        .file_name()
        .map(|value| value.to_string_lossy().into_owned());
    let safe_name = safe_attachment_filename(
        selected_name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(Some(default_safe_name)),
    );
    let mut publication = AtomicNoClobberFile::new_in(directory)?;
    publication.stage(|temporary_file| temporary_file.write_all(bytes))?;

    for collision_index in 0..=10_000 {
        let candidate_name = attachment_name_candidate(&safe_name, collision_index);
        match publication.try_publish(candidate_name.as_ref())? {
            PublishAttempt::Published => return Ok(candidate_name),
            PublishAttempt::Occupied => continue,
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no non-colliding attachment destination is available",
    ))
}

fn publish_managed_blob(
    publication: &mut AtomicNoClobberFile,
    mut next_id: impl FnMut() -> String,
) -> Result<(String, String)> {
    for _ in 0..32 {
        let id = next_id();
        let internal_name = format!("{id}.blob");
        validate_internal_name(&internal_name)?;
        match publication.try_publish(internal_name.as_ref())? {
            PublishAttempt::Published => return Ok((id, internal_name)),
            PublishAttempt::Occupied => continue,
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique managed attachment identifier",
    )
    .into())
}

fn validate_internal_name(internal_name: &str) -> Result<()> {
    let path = Path::new(internal_name);
    let valid_component = {
        let mut components = path.components();
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
    };
    let opaque_id = internal_name.strip_suffix(".blob");
    if !valid_component
        || internal_name.len() > 80
        || opaque_id.is_none_or(|value| Uuid::parse_str(value).is_err())
    {
        return Err(MailError::Validation(
            "the managed attachment identifier is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn account_scope_component(stable_account_id: &str) -> Result<String> {
    let stable_account_id = stable_account_id.trim();
    if stable_account_id.is_empty() || stable_account_id.len() > 1024 {
        return Err(MailError::Validation(
            "managed attachment storage requires a bounded stable account id".to_owned(),
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(b"mine-mail-managed-attachment-account-scope-v1\0");
    hasher.update((stable_account_id.len() as u64).to_be_bytes());
    hasher.update(stable_account_id.as_bytes());
    let digest = hasher.finalize();
    let mut component = String::with_capacity("account-".len() + digest.len() * 2);
    component.push_str("account-");
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        component.push(char::from(HEX[usize::from(byte >> 4)]));
        component.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(component)
}

fn normalize_mime_type(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    let valid = value.len() <= 255
        && value
            .split_once('/')
            .is_some_and(|(major, minor)| !major.is_empty() && !minor.is_empty())
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | '-' | '+' | '.' | '_')
        });
    if valid {
        value
    } else {
        "application/octet-stream".to_owned()
    }
}

fn mime_type_for_name(name: &str) -> &'static str {
    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    match extension.to_ascii_lowercase().as_str() {
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "eml" => "message/rfc822",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        fs,
        io::{self, Cursor, Read, Write},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, SystemTime},
    };

    use tempfile::tempdir;

    use crate::atomic_publish::AtomicNoClobberFile;

    use super::{
        ManagedAttachmentStore, account_scope_component, publish_managed_blob, read_bounded_exact,
        save_extracted_file, validate_account_storage_target, validate_internal_name,
    };

    #[cfg(unix)]
    fn create_directory_link(target: &std::path::Path, link: &std::path::Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_directory_link(target: &std::path::Path, link: &std::path::Path) -> io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    #[cfg(unix)]
    fn create_file_link(target: &std::path::Path, link: &std::path::Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_file_link(target: &std::path::Path, link: &std::path::Path) -> io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    fn create_link_or_skip(result: io::Result<()>) -> bool {
        match result {
            Ok(()) => true,
            #[cfg(windows)]
            Err(error)
                if error.raw_os_error() == Some(1314)
                    || matches!(
                        error.kind(),
                        io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                    ) =>
            {
                false
            }
            Err(error) => panic!("create test reparse link: {error}"),
        }
    }

    struct FailingReader {
        first: bool,
    }

    impl Read for FailingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.first {
                self.first = false;
                buffer[..3].copy_from_slice(b"abc");
                Ok(3)
            } else {
                Err(io::Error::other("synthetic partial read failure"))
            }
        }
    }

    struct GrowingReader {
        emitted: Arc<AtomicUsize>,
    }

    impl Read for GrowingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = buffer.len().min(1024);
            buffer[..count].fill(b'x');
            self.emitted.fetch_add(count, Ordering::SeqCst);
            Ok(count)
        }
    }

    #[test]
    fn account_scope_sha256_is_deterministic_distinct_and_path_safe() {
        let first = account_scope_component("stable-account-a").unwrap();
        assert_eq!(first, account_scope_component("stable-account-a").unwrap());
        assert_ne!(first, account_scope_component("stable-account-b").unwrap());
        assert_eq!(first.len(), "account-".len() + 64);
        assert!(first.starts_with("account-"));
        assert!(
            first["account-".len()..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        );
        assert!(!first.contains("stable-account-a"));
        assert_eq!(std::path::Path::new(&first).components().count(), 1);
    }

    #[test]
    fn import_sanitizes_names_and_uses_generic_unknown_mime() {
        let root = tempdir().unwrap();
        let store = ManagedAttachmentStore::new(root.path(), "account-a").unwrap();
        let imported = store
            .import_reader(
                Cursor::new(b"content"),
                "../../CON.unknown. ",
                "not a mime",
                7,
            )
            .unwrap();
        assert!(!imported.name.contains(['/', '\\']));
        assert!(!imported.name.ends_with(['.', ' ']));
        assert_eq!(imported.mime_type, "application/octet-stream");
        assert_eq!(imported.size_bytes, 7);
        assert_eq!(
            imported.sha256_hex,
            "ed7002b439e9ac845f22357d822bac1444730fbdb6016d3ec\
             9432297b9ec9f73"
                .replace(char::is_whitespace, "")
        );
        assert_eq!(
            fs::read(store.internal_path(&imported.internal_name).unwrap()).unwrap(),
            b"content"
        );
    }

    #[test]
    fn selected_file_is_copied_with_exact_size_and_no_source_path_reference() {
        let root = tempdir().unwrap();
        let source_directory = tempdir().unwrap();
        let source = source_directory.path().join("report.pdf");
        fs::write(&source, b"%PDF-test").unwrap();
        let store = ManagedAttachmentStore::new(root.path(), "account-a").unwrap();
        let imported = store.import_file(&source).unwrap();
        assert_eq!(imported.name, "report.pdf");
        assert_eq!(imported.mime_type, "application/pdf");
        assert_eq!(imported.size_bytes, 9);
        assert!(!imported.internal_name.contains("report"));
        let mut bytes = Vec::new();
        store
            .open_internal_file(&imported.internal_name)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, b"%PDF-test");
        assert!(store.remove_internal_file(&imported.internal_name).unwrap());
        assert!(!store.remove_internal_file(&imported.internal_name).unwrap());
    }

    #[test]
    fn account_scoped_cleanup_never_scans_another_accounts_blobs() {
        let root = tempdir().unwrap();
        let first = ManagedAttachmentStore::new(root.path(), "stable-account-a").unwrap();
        let second = ManagedAttachmentStore::new(root.path(), "stable-account-b").unwrap();
        assert_ne!(first.blob_root, second.blob_root);
        assert_eq!(first.blob_root.parent(), second.blob_root.parent());
        let first_blob = first
            .import_bytes(b"first", "first.bin", "application/octet-stream")
            .unwrap();
        let second_blob = second
            .import_bytes(b"second", "second.bin", "application/octet-stream")
            .unwrap();

        assert_eq!(
            first
                .cleanup_unregistered_files(&HashSet::new(), Duration::ZERO)
                .unwrap(),
            1
        );
        assert!(
            !first
                .internal_path(&first_blob.internal_name)
                .unwrap()
                .exists()
        );
        assert_eq!(
            second
                .read_internal_file(
                    &second_blob.internal_name,
                    second_blob.size_bytes,
                    &second_blob.sha256_hex,
                )
                .unwrap(),
            b"second"
        );
    }

    #[test]
    fn concurrent_store_cleanup_keeps_new_temporary_files_and_removes_old_ones() {
        let root = tempdir().unwrap();
        let local = ManagedAttachmentStore::new(root.path(), "stable-account").unwrap();
        let network = ManagedAttachmentStore::new(root.path(), "stable-account").unwrap();
        assert_eq!(local.blob_root, network.blob_root);
        let active = local.blob_root.join(".tmp-active-import");
        fs::write(&active, b"in progress").unwrap();

        assert_eq!(
            network
                .cleanup_temporary_files(Duration::from_secs(60 * 60))
                .unwrap(),
            0
        );
        assert!(active.exists());
        assert_eq!(
            network
                .cleanup_temporary_files_at(
                    Duration::from_secs(60 * 60),
                    SystemTime::now() + Duration::from_secs(2 * 60 * 60),
                )
                .unwrap(),
            1
        );
        assert!(!active.exists());
    }

    #[test]
    fn managed_atomic_temporary_names_are_owned_by_crash_cleanup() {
        let root = tempdir().unwrap();
        let store = ManagedAttachmentStore::new(root.path(), "stable-account").unwrap();
        let residual_name = {
            let publication =
                AtomicNoClobberFile::new_managed_attachment_in(&store.blob_root).unwrap();
            publication.temporary_path().file_name().unwrap().to_owned()
        };
        assert!(residual_name.to_string_lossy().starts_with(".tmp-"));
        assert!(!residual_name.to_string_lossy().starts_with(".mine-mail-"));
        let residual_path = store.blob_root.join(residual_name);
        fs::write(&residual_path, b"complete interrupted staging").unwrap();

        assert_eq!(store.cleanup_temporary_files(Duration::ZERO).unwrap(), 1);
        assert!(!residual_path.exists());
    }

    #[test]
    fn partial_import_removes_temporary_output() {
        let root = tempdir().unwrap();
        let store = ManagedAttachmentStore::new(root.path(), "account-a").unwrap();
        assert!(
            store
                .import_reader(FailingReader { first: true }, "note.txt", "text/plain", 8)
                .is_err()
        );
        assert!(store.list_internal_names().unwrap().is_empty());
        assert_eq!(store.cleanup_temporary_files(Duration::ZERO).unwrap(), 0);
    }

    #[test]
    fn growing_import_is_bounded_to_the_expected_size_plus_one() {
        let root = tempdir().unwrap();
        let store = ManagedAttachmentStore::new(root.path(), "account-a").unwrap();
        let emitted = Arc::new(AtomicUsize::new(0));

        assert!(
            store
                .import_reader(
                    GrowingReader {
                        emitted: Arc::clone(&emitted),
                    },
                    "growing.bin",
                    "application/octet-stream",
                    3,
                )
                .is_err()
        );
        assert_eq!(emitted.load(Ordering::SeqCst), 4);
        assert!(store.list_internal_names().unwrap().is_empty());
        assert_eq!(store.cleanup_temporary_files(Duration::ZERO).unwrap(), 0);
    }

    #[test]
    fn growing_managed_read_is_bounded_to_the_expected_size_plus_one() {
        let emitted = Arc::new(AtomicUsize::new(0));
        assert!(
            read_bounded_exact(
                GrowingReader {
                    emitted: Arc::clone(&emitted),
                },
                3,
            )
            .is_err()
        );
        assert_eq!(emitted.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn equal_length_blob_replacement_fails_the_persisted_digest_check() {
        let root = tempdir().unwrap();
        let store = ManagedAttachmentStore::new(root.path(), "account-a").unwrap();
        let imported = store
            .import_bytes(b"original", "evidence.bin", "application/octet-stream")
            .unwrap();
        let path = store.internal_path(&imported.internal_name).unwrap();
        fs::write(&path, b"tampered").unwrap();

        assert!(
            store
                .read_internal_file(
                    &imported.internal_name,
                    imported.size_bytes,
                    &imported.sha256_hex,
                )
                .is_err()
        );
    }

    #[test]
    fn account_delete_target_must_be_the_exact_direct_child() {
        let managed = std::path::Path::new("managed-attachments");
        let expected = managed.join("account-a");
        assert!(validate_account_storage_target(&expected, &expected, managed).is_ok());
        assert!(
            validate_account_storage_target(&managed.join("account-b"), &expected, managed)
                .is_err()
        );
        assert!(
            validate_account_storage_target(
                &expected.join("nested"),
                &expected.join("nested"),
                managed
            )
            .is_err()
        );
        assert!(
            validate_account_storage_target(
                std::path::Path::new("outside"),
                std::path::Path::new("outside"),
                managed
            )
            .is_err()
        );
    }

    #[test]
    fn account_scope_link_to_another_account_is_rejected_without_touching_it() {
        let root = tempdir().unwrap();
        let other = ManagedAttachmentStore::new(root.path(), "account-b").unwrap();
        let sentinel = other
            .import_bytes(b"other account", "other.bin", "application/octet-stream")
            .unwrap();
        let linked_scope = other
            .managed_root
            .join(account_scope_component("account-a").unwrap());
        if !create_link_or_skip(create_directory_link(&other.blob_root, &linked_scope)) {
            return;
        }

        assert!(ManagedAttachmentStore::new(root.path(), "account-a").is_err());
        assert_eq!(
            other
                .read_internal_file(
                    &sentinel.internal_name,
                    sentinel.size_bytes,
                    &sentinel.sha256_hex
                )
                .unwrap(),
            b"other account"
        );
    }

    #[test]
    fn account_scope_link_to_external_directory_is_rejected_without_touching_it() {
        let root = tempdir().unwrap();
        let external = tempdir().unwrap();
        let sentinel = external.path().join("sentinel");
        fs::write(&sentinel, b"external").unwrap();
        let managed_root = root.path().join("managed-attachments");
        fs::create_dir_all(&managed_root).unwrap();
        let linked_scope = managed_root.join(account_scope_component("account-a").unwrap());
        if !create_link_or_skip(create_directory_link(external.path(), &linked_scope)) {
            return;
        }

        assert!(ManagedAttachmentStore::new(root.path(), "account-a").is_err());
        assert_eq!(fs::read(sentinel).unwrap(), b"external");
    }

    #[test]
    fn runtime_account_scope_replacement_is_rejected_without_deleting_the_target() {
        let root = tempdir().unwrap();
        let first = ManagedAttachmentStore::new(root.path(), "account-a").unwrap();
        let other = ManagedAttachmentStore::new(root.path(), "account-b").unwrap();
        let sentinel = other
            .import_bytes(b"other account", "other.bin", "application/octet-stream")
            .unwrap();
        fs::remove_dir(&first.blob_root).unwrap();
        if !create_link_or_skip(create_directory_link(&other.blob_root, &first.blob_root)) {
            return;
        }

        assert!(first.list_internal_names().is_err());
        assert!(first.delete_account_storage().is_err());
        assert_eq!(
            other
                .read_internal_file(
                    &sentinel.internal_name,
                    sentinel.size_bytes,
                    &sentinel.sha256_hex
                )
                .unwrap(),
            b"other account"
        );
    }

    #[test]
    fn blob_link_is_rejected_without_reading_or_removing_its_external_target() {
        let root = tempdir().unwrap();
        let external = tempdir().unwrap();
        let store = ManagedAttachmentStore::new(root.path(), "account-a").unwrap();
        let imported = store
            .import_bytes(b"original", "evidence.bin", "application/octet-stream")
            .unwrap();
        let blob_path = store.internal_path(&imported.internal_name).unwrap();
        fs::remove_file(&blob_path).unwrap();
        let external_file = external.path().join("external.bin");
        fs::write(&external_file, b"external").unwrap();
        if !create_link_or_skip(create_file_link(&external_file, &blob_path)) {
            return;
        }

        assert!(
            store
                .read_internal_file(
                    &imported.internal_name,
                    imported.size_bytes,
                    &imported.sha256_hex
                )
                .is_err()
        );
        assert!(store.remove_internal_file(&imported.internal_name).is_err());
        assert!(store.list_internal_names().is_err());
        assert_eq!(fs::read(external_file).unwrap(), b"external");
    }

    #[test]
    fn managed_blob_collision_preserves_existing_content_and_retries_uuid() {
        let root = tempdir().unwrap();
        let occupied_id = "018f0000-0000-7000-8000-000000000001";
        let available_id = "018f0000-0000-7000-8000-000000000002";
        let occupied_name = format!("{occupied_id}.blob");
        fs::write(root.path().join(&occupied_name), b"old").unwrap();
        let mut publication = AtomicNoClobberFile::new_managed_attachment_in(root.path()).unwrap();
        publication.stage(|file| file.write_all(b"new")).unwrap();
        let mut candidates = [occupied_id, available_id].into_iter();

        let (saved_id, saved_name) =
            publish_managed_blob(&mut publication, || candidates.next().unwrap().to_owned())
                .unwrap();

        assert_eq!(saved_id, available_id);
        assert_eq!(saved_name, format!("{available_id}.blob"));
        assert_eq!(fs::read(root.path().join(occupied_name)).unwrap(), b"old");
        assert_eq!(fs::read(root.path().join(saved_name)).unwrap(), b"new");
        assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-")
        }));
    }

    #[test]
    fn save_as_uses_a_safe_collision_name_without_partial_output() {
        let root = tempdir().unwrap();
        let requested = root.path().join("report.pdf");
        fs::write(&requested, b"existing").unwrap();

        let saved_name = save_extracted_file(&requested, "default.pdf", b"new content").unwrap();

        assert_eq!(saved_name, "report (1).pdf");
        assert_eq!(fs::read(&requested).unwrap(), b"existing");
        assert_eq!(
            fs::read(root.path().join(&saved_name)).unwrap(),
            b"new content"
        );
        assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".mine-mail-")
        }));
    }

    #[test]
    fn managed_identifiers_cannot_escape_the_blob_root() {
        for invalid in [
            "../value.blob",
            "folder/value.blob",
            "folder\\value.blob",
            "C:value.blob",
            "not-a-uuid.blob",
            ".tmp-value",
        ] {
            assert!(validate_internal_name(invalid).is_err(), "{invalid}");
        }
    }
}
