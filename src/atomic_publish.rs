//! Same-directory, no-clobber publication for complete files.
//!
//! `tempfile::NamedTempFile::persist_noclobber` selects the platform's
//! no-replace rename where available and retains a no-clobber hard-link
//! fallback on Unix. Publication is the commit point: after it succeeds, this
//! module performs no fallible cleanup and never removes the final name.
//!
//! This remains a path-based primitive. Rejecting a selected directory whose
//! final component is a symlink or Windows reparse point narrows accidental
//! redirection, but does not anchor later operations to an opened directory
//! handle. A hostile same-user directory replacement race is therefore a
//! residual limitation and belongs in the native release matrix.

use std::{
    ffi::OsStr,
    fs::{self, File, Metadata},
    io,
    path::{Component, Path, PathBuf},
};

use tempfile::{Builder, NamedTempFile};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublishAttempt {
    Published,
    Occupied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PublishState {
    Fresh,
    Ready,
    Poisoned,
    Published,
}

pub(crate) struct AtomicNoClobberFile {
    directory: PathBuf,
    pending: Option<NamedTempFile>,
    state: PublishState,
}

impl AtomicNoClobberFile {
    pub(crate) fn new_in(directory: &Path) -> io::Result<Self> {
        Self::new_with_naming(directory, ".mine-mail-", ".tmp")
    }

    /// Uses the prefix already recognized by managed-attachment crash cleanup.
    pub(crate) fn new_managed_attachment_in(directory: &Path) -> io::Result<Self> {
        Self::new_with_naming(directory, ".tmp-", "")
    }

    fn new_with_naming(directory: &Path, prefix: &str, suffix: &str) -> io::Result<Self> {
        let directory = canonical_real_directory(directory)?;
        let pending = Builder::new()
            .prefix(prefix)
            .suffix(suffix)
            .tempfile_in(&directory)?;
        Ok(Self {
            directory,
            pending: Some(pending),
            state: PublishState::Fresh,
        })
    }

    /// Writes and synchronizes all content while only the hidden temporary
    /// sibling exists. A failed writer poisons the object so partial bytes can
    /// only be removed by `NamedTempFile`'s drop cleanup.
    pub(crate) fn stage<E>(
        &mut self,
        writer: impl FnOnce(&mut File) -> Result<(), E>,
    ) -> Result<(), E>
    where
        E: From<io::Error>,
    {
        if self.state != PublishState::Fresh {
            return Err(E::from(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the temporary file is not available for staging",
            )));
        }

        let result = {
            let pending = self
                .pending
                .as_mut()
                .expect("a fresh atomic publication owns its temporary file");
            writer(pending.as_file_mut())
                .and_then(|()| pending.as_file().sync_all().map_err(E::from))
        };

        match result {
            Ok(()) => {
                self.state = PublishState::Ready;
                Ok(())
            }
            Err(error) => {
                self.state = PublishState::Poisoned;
                Err(error)
            }
        }
    }

    /// Publishes under one base name without replacing an existing entry.
    ///
    /// An occupied name leaves the staged temporary file intact so the caller
    /// can atomically try a collision suffix. Success is the commit point.
    pub(crate) fn try_publish(&mut self, base_name: &OsStr) -> io::Result<PublishAttempt> {
        if self.state != PublishState::Ready {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the temporary file is not ready for publication",
            ));
        }
        validate_base_name(base_name)?;

        let final_path = self.directory.join(base_name);
        let pending = self
            .pending
            .take()
            .expect("a ready atomic publication owns its temporary file");
        match pending.persist_noclobber(final_path) {
            Ok(published_file) => {
                self.state = PublishState::Published;
                drop(published_file);
                Ok(PublishAttempt::Published)
            }
            Err(error) => {
                let kind = error.error.kind();
                self.pending = Some(error.file);
                if kind == io::ErrorKind::AlreadyExists {
                    Ok(PublishAttempt::Occupied)
                } else {
                    Err(error.error)
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn temporary_path(&self) -> &Path {
        self.pending
            .as_ref()
            .expect("an unpublished atomic file owns its temporary path")
            .path()
    }
}

fn canonical_real_directory(directory: &Path) -> io::Result<PathBuf> {
    if !directory.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the publication directory must be absolute",
        ));
    }

    validate_real_directory(directory)?;
    let canonical = fs::canonicalize(directory)?;
    validate_real_directory(&canonical)?;
    Ok(canonical)
}

fn validate_real_directory(directory: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(directory)?;
    if metadata_is_reparse(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the publication directory cannot be a link or reparse point",
        ));
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "the publication destination is not a directory",
        ));
    }
    Ok(())
}

fn validate_base_name(base_name: &OsStr) -> io::Result<()> {
    let mut components = Path::new(base_name).components();
    let is_base_name =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if !is_base_name {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the publication target must be one base name",
        ));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{self, Write},
        sync::{Arc, Barrier},
        thread,
    };

    use tempfile::tempdir;

    use super::{AtomicNoClobberFile, PublishAttempt};

    #[test]
    fn final_name_appears_only_after_synchronized_staging() {
        let root = tempdir().unwrap();
        let final_path = root.path().join("report.pdf");
        let mut publication = AtomicNoClobberFile::new_in(root.path()).unwrap();

        publication
            .stage(|file| file.write_all(b"complete content"))
            .unwrap();
        assert!(!final_path.exists());

        assert_eq!(
            publication.try_publish("report.pdf".as_ref()).unwrap(),
            PublishAttempt::Published
        );
        assert_eq!(fs::read(final_path).unwrap(), b"complete content");
        assert_no_temporary_siblings(root.path());
    }

    #[test]
    fn occupied_target_is_preserved_and_the_same_staging_file_can_retry() {
        let root = tempdir().unwrap();
        let occupied = root.path().join("report.pdf");
        fs::write(&occupied, b"existing").unwrap();
        let mut publication = AtomicNoClobberFile::new_in(root.path()).unwrap();
        publication
            .stage(|file| file.write_all(b"new content"))
            .unwrap();

        assert_eq!(
            publication.try_publish("report.pdf".as_ref()).unwrap(),
            PublishAttempt::Occupied
        );
        assert_eq!(fs::read(&occupied).unwrap(), b"existing");
        assert_eq!(
            publication.try_publish("report (1).pdf".as_ref()).unwrap(),
            PublishAttempt::Published
        );
        assert_eq!(
            fs::read(root.path().join("report (1).pdf")).unwrap(),
            b"new content"
        );
    }

    #[test]
    fn managed_staging_uses_the_crash_cleanup_prefix() {
        let root = tempdir().unwrap();
        let publication = AtomicNoClobberFile::new_managed_attachment_in(root.path()).unwrap();
        let name = publication
            .temporary_path()
            .file_name()
            .unwrap()
            .to_string_lossy();

        assert!(name.starts_with(".tmp-"));
        assert!(!name.starts_with(".mine-mail-"));
    }

    #[test]
    fn failed_writer_leaves_no_final_and_drop_removes_partial_staging() {
        let root = tempdir().unwrap();
        let temporary_path;
        {
            let mut publication = AtomicNoClobberFile::new_in(root.path()).unwrap();
            temporary_path = publication.temporary_path().to_owned();
            let error = publication
                .stage(|file| {
                    file.write_all(b"partial")?;
                    Err(io::Error::other("simulated writer failure"))
                })
                .unwrap_err();

            assert_eq!(error.kind(), io::ErrorKind::Other);
            assert!(temporary_path.exists());
            assert!(!root.path().join("report.pdf").exists());
            assert!(publication.try_publish("report.pdf".as_ref()).is_err());
        }
        assert!(!temporary_path.exists());
        assert_no_temporary_siblings(root.path());
    }

    #[test]
    fn concurrent_publication_has_exactly_one_complete_winner() {
        const WRITERS: usize = 8;

        let root = tempdir().unwrap();
        let root_path = Arc::new(root.path().to_owned());
        let barrier = Arc::new(Barrier::new(WRITERS));
        let mut workers = Vec::new();

        for index in 0..WRITERS {
            let root_path = Arc::clone(&root_path);
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                let payload = format!("complete payload {index}").into_bytes();
                let mut publication = AtomicNoClobberFile::new_in(&root_path).unwrap();
                publication.stage(|file| file.write_all(&payload)).unwrap();
                barrier.wait();
                let attempt = publication.try_publish("shared.bin".as_ref()).unwrap();
                (attempt, payload)
            }));
        }

        let outcomes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes
                .iter()
                .filter(|(attempt, _)| *attempt == PublishAttempt::Published)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|(attempt, _)| *attempt == PublishAttempt::Occupied)
                .count(),
            WRITERS - 1
        );
        let final_bytes = fs::read(root.path().join("shared.bin")).unwrap();
        assert!(outcomes.iter().any(|(_, payload)| payload == &final_bytes));
        assert_no_temporary_siblings(root.path());
    }

    #[test]
    fn invalid_base_names_and_unstaged_publication_are_rejected() {
        let root = tempdir().unwrap();
        let mut publication = AtomicNoClobberFile::new_in(root.path()).unwrap();
        assert!(publication.try_publish("report.pdf".as_ref()).is_err());
        publication
            .stage(|file| file.write_all(b"complete"))
            .unwrap();

        for invalid in ["", ".", "..", "../report.pdf", "folder/report.pdf"] {
            assert!(
                publication.try_publish(invalid.as_ref()).is_err(),
                "{invalid}"
            );
        }
        assert!(!root.path().join("report.pdf").exists());
    }

    #[cfg(unix)]
    #[test]
    fn linked_directory_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        let target = root.path().join("target");
        let linked = root.path().join("linked");
        fs::create_dir(&target).unwrap();
        symlink(&target, &linked).unwrap();

        assert!(AtomicNoClobberFile::new_in(&linked).is_err());
        assert_no_temporary_siblings(&target);
    }

    #[cfg(windows)]
    #[test]
    fn linked_directory_is_rejected_when_symlink_creation_is_available() {
        use std::os::windows::fs::symlink_dir;

        let root = tempdir().unwrap();
        let target = root.path().join("target");
        let linked = root.path().join("linked");
        fs::create_dir(&target).unwrap();
        if symlink_dir(&target, &linked).is_err() {
            return;
        }

        assert!(AtomicNoClobberFile::new_in(&linked).is_err());
        assert_no_temporary_siblings(&target);
    }

    fn assert_no_temporary_siblings(directory: &std::path::Path) {
        assert!(fs::read_dir(directory).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".mine-mail-")
        }));
    }
}
