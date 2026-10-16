//! Deterministic source inventory for forward-backward matrix reports.

use core::fmt;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

const SOURCE_DOMAIN: &[u8] = b"ssfilt.forward-backward.source\0";
const SOURCE_SCHEMA: u8 = 1;
const MANIFEST_PATH: &str = "report/manifest.csv";

/// Whether an inventory entry was read from the repository or supplied by the
/// report generator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceEntryKind {
    Repository,
    Synthetic,
}

impl SourceEntryKind {
    /// Stable spelling used by the report writer.
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Repository => "repository",
            Self::Synthetic => "synthetic",
        }
    }
}

/// One source-file identity included in a report's provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceEntry {
    relative_path: String,
    kind: SourceEntryKind,
    byte_length: u64,
    sha256: [u8; 32],
}

impl SourceEntry {
    #[must_use]
    pub(crate) fn relative_path(&self) -> &str {
        &self.relative_path
    }

    #[must_use]
    pub(crate) const fn kind(&self) -> SourceEntryKind {
        self.kind
    }

    #[must_use]
    pub(crate) const fn byte_length(&self) -> u64 {
        self.byte_length
    }

    #[must_use]
    pub(crate) const fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    #[must_use]
    pub(crate) fn sha256_hex(&self) -> String {
        lowercase_hex(self.sha256())
    }
}

/// Complete schema-v1 source identity for one canonical manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceProvenance {
    entries: Vec<SourceEntry>,
    source_digest: [u8; 32],
}

impl SourceProvenance {
    /// Repository entries in bytewise path order, followed by the synthetic
    /// manifest entry.
    #[must_use]
    pub(crate) fn entries(&self) -> &[SourceEntry] {
        &self.entries
    }

    #[must_use]
    pub(crate) const fn source_digest(&self) -> &[u8; 32] {
        &self.source_digest
    }

    #[must_use]
    pub(crate) fn source_digest_hex(&self) -> String {
        lowercase_hex(self.source_digest())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UnsupportedPathKind {
    Symlink,
    Directory,
    Other,
}

/// Failure while locating, inventorying, or hashing report source inputs.
#[derive(Debug)]
pub(crate) enum ProvenanceError {
    RepositoryRootUnavailable,
    MissingRequiredFile {
        relative_path: String,
    },
    MissingRequiredDirectory {
        relative_path: String,
    },
    UnsupportedPath {
        relative_path: String,
        kind: UnsupportedPathKind,
    },
    NonUtf8Path {
        path: PathBuf,
    },
    PathOutsideRepository {
        path: PathBuf,
    },
    DuplicatePath {
        relative_path: String,
    },
    LengthTooLarge {
        relative_path: String,
        byte_length: usize,
    },
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for ProvenanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RepositoryRootUnavailable => write!(
                formatter,
                "oracle manifest directory has no tools parent and repository parent"
            ),
            Self::MissingRequiredFile { relative_path } => {
                write!(
                    formatter,
                    "required provenance file is missing: {relative_path}"
                )
            }
            Self::MissingRequiredDirectory { relative_path } => write!(
                formatter,
                "required provenance directory is missing: {relative_path}"
            ),
            Self::UnsupportedPath {
                relative_path,
                kind,
            } => write!(
                formatter,
                "unsupported {kind:?} in provenance source closure: {relative_path}"
            ),
            Self::NonUtf8Path { path } => {
                write!(
                    formatter,
                    "provenance path is not UTF-8: {}",
                    path.display()
                )
            }
            Self::PathOutsideRepository { path } => write!(
                formatter,
                "provenance path is outside the repository: {}",
                path.display()
            ),
            Self::DuplicatePath { relative_path } => {
                write!(formatter, "duplicate provenance path: {relative_path}")
            }
            Self::LengthTooLarge {
                relative_path,
                byte_length,
            } => write!(
                formatter,
                "provenance entry {relative_path} has {byte_length} bytes, which exceeds u64"
            ),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "could not {operation} provenance path {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ProvenanceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Inventory the repository containing this oracle tool and include the
/// supplied canonical manifest in the aggregate source digest.
pub(crate) fn collect_source_provenance(
    canonical_manifest: &[u8],
) -> Result<SourceProvenance, ProvenanceError> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repository_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or(ProvenanceError::RepositoryRootUnavailable)?;
    collect_source_provenance_from_root(repository_root, canonical_manifest)
}

/// Testable implementation whose root is the directory containing the root
/// `Cargo.toml`, `src`, `docs`, and `tools` entries.
fn collect_source_provenance_from_root(
    repository_root: &Path,
    canonical_manifest: &[u8],
) -> Result<SourceProvenance, ProvenanceError> {
    let mut relative_paths = Vec::new();

    add_required_file(repository_root, "Cargo.toml", &mut relative_paths)?;
    add_required_file(repository_root, "Cargo.lock", &mut relative_paths)?;
    add_rs_tree(repository_root, "src", &mut relative_paths)?;

    add_required_file(
        repository_root,
        "tools/forward_backward_protocol/Cargo.toml",
        &mut relative_paths,
    )?;
    add_optional_file(
        repository_root,
        "tools/forward_backward_protocol/Cargo.lock",
        &mut relative_paths,
    )?;
    add_required_file(
        repository_root,
        "tools/forward_backward_protocol/src/lib.rs",
        &mut relative_paths,
    )?;

    add_required_file(
        repository_root,
        "tools/forward_backward_oracle/Cargo.toml",
        &mut relative_paths,
    )?;
    add_required_file(
        repository_root,
        "tools/forward_backward_oracle/Cargo.lock",
        &mut relative_paths,
    )?;
    add_required_file(
        repository_root,
        "tools/forward_backward_oracle/README.md",
        &mut relative_paths,
    )?;
    add_rs_tree(
        repository_root,
        "tools/forward_backward_oracle/src",
        &mut relative_paths,
    )?;

    add_required_file(
        repository_root,
        "docs/forward-backward.md",
        &mut relative_paths,
    )?;
    add_required_file(
        repository_root,
        "docs/forward-backward-fixtures.md",
        &mut relative_paths,
    )?;

    relative_paths.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    for pair in relative_paths.windows(2) {
        if pair[0] == pair[1] {
            return Err(ProvenanceError::DuplicatePath {
                relative_path: pair[0].clone(),
            });
        }
    }

    let mut aggregate = Sha256::new();
    aggregate.update(SOURCE_DOMAIN);
    aggregate.update([SOURCE_SCHEMA]);

    let mut entries = Vec::new();
    entries
        .try_reserve(relative_paths.len().saturating_add(1))
        .map_err(|source| ProvenanceError::Io {
            operation: "reserve memory for",
            path: repository_root.to_path_buf(),
            source: io::Error::other(source),
        })?;

    for relative_path in relative_paths {
        let absolute_path = repository_root.join(Path::new(&relative_path));
        require_regular_file(repository_root, &absolute_path, &relative_path)?;
        let contents = fs::read(&absolute_path).map_err(|source| ProvenanceError::Io {
            operation: "read",
            path: absolute_path,
            source,
        })?;
        let entry = make_entry(relative_path, SourceEntryKind::Repository, &contents)?;
        frame_entry(&mut aggregate, entry.relative_path(), &contents)?;
        entries.push(entry);
    }

    frame_entry(&mut aggregate, MANIFEST_PATH, canonical_manifest)?;
    entries.push(make_entry(
        MANIFEST_PATH.to_owned(),
        SourceEntryKind::Synthetic,
        canonical_manifest,
    )?);

    Ok(SourceProvenance {
        entries,
        source_digest: aggregate.finalize().into(),
    })
}

fn add_required_file(
    repository_root: &Path,
    relative_path: &str,
    paths: &mut Vec<String>,
) -> Result<(), ProvenanceError> {
    let absolute_path = repository_root.join(Path::new(relative_path));
    require_regular_file(repository_root, &absolute_path, relative_path)?;
    paths.push(relative_path.to_owned());
    Ok(())
}

fn add_optional_file(
    repository_root: &Path,
    relative_path: &str,
    paths: &mut Vec<String>,
) -> Result<(), ProvenanceError> {
    let absolute_path = repository_root.join(Path::new(relative_path));
    match fs::symlink_metadata(&absolute_path) {
        Ok(_) => {
            require_regular_file(repository_root, &absolute_path, relative_path)?;
            paths.push(relative_path.to_owned());
            Ok(())
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ProvenanceError::Io {
            operation: "inspect",
            path: absolute_path,
            source,
        }),
    }
}

fn add_rs_tree(
    repository_root: &Path,
    relative_directory: &str,
    paths: &mut Vec<String>,
) -> Result<(), ProvenanceError> {
    let absolute_directory = repository_root.join(Path::new(relative_directory));
    require_directory(repository_root, &absolute_directory, relative_directory)?;
    visit_rs_tree(repository_root, &absolute_directory, paths)
}

fn visit_rs_tree(
    repository_root: &Path,
    directory: &Path,
    paths: &mut Vec<String>,
) -> Result<(), ProvenanceError> {
    let entries = fs::read_dir(directory).map_err(|source| ProvenanceError::Io {
        operation: "read directory",
        path: directory.to_path_buf(),
        source,
    })?;

    for entry_result in entries {
        let entry = entry_result.map_err(|source| ProvenanceError::Io {
            operation: "read directory entry in",
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let relative_path = relative_path(repository_root, &path)?;
        let file_type = entry.file_type().map_err(|source| ProvenanceError::Io {
            operation: "inspect",
            path: path.clone(),
            source,
        })?;

        if file_type.is_symlink() {
            return Err(ProvenanceError::UnsupportedPath {
                relative_path,
                kind: UnsupportedPathKind::Symlink,
            });
        }
        if file_type.is_dir() {
            visit_rs_tree(repository_root, &path, paths)?;
        } else if file_type.is_file() {
            if path.extension().is_some_and(|extension| extension == "rs") {
                paths.push(relative_path);
            }
        } else {
            return Err(ProvenanceError::UnsupportedPath {
                relative_path,
                kind: UnsupportedPathKind::Other,
            });
        }
    }

    Ok(())
}

fn require_directory(
    repository_root: &Path,
    absolute_path: &Path,
    relative_path_hint: &str,
) -> Result<(), ProvenanceError> {
    match fs::symlink_metadata(absolute_path) {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                Err(ProvenanceError::UnsupportedPath {
                    relative_path: relative_path_hint.to_owned(),
                    kind: UnsupportedPathKind::Symlink,
                })
            } else if file_type.is_dir() {
                Ok(())
            } else {
                let normalized = relative_path(repository_root, absolute_path)?;
                Err(ProvenanceError::UnsupportedPath {
                    relative_path: normalized,
                    kind: UnsupportedPathKind::Other,
                })
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Err(ProvenanceError::MissingRequiredDirectory {
                relative_path: relative_path_hint.to_owned(),
            })
        }
        Err(source) => Err(ProvenanceError::Io {
            operation: "inspect",
            path: absolute_path.to_path_buf(),
            source,
        }),
    }
}

fn require_regular_file(
    repository_root: &Path,
    absolute_path: &Path,
    relative_path_hint: &str,
) -> Result<(), ProvenanceError> {
    match fs::symlink_metadata(absolute_path) {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            if file_type.is_file() && !file_type.is_symlink() {
                Ok(())
            } else {
                let relative_path = relative_path(repository_root, absolute_path)?;
                let kind = if file_type.is_symlink() {
                    UnsupportedPathKind::Symlink
                } else if file_type.is_dir() {
                    UnsupportedPathKind::Directory
                } else {
                    UnsupportedPathKind::Other
                };
                Err(ProvenanceError::UnsupportedPath {
                    relative_path,
                    kind,
                })
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Err(ProvenanceError::MissingRequiredFile {
                relative_path: relative_path_hint.to_owned(),
            })
        }
        Err(source) => Err(ProvenanceError::Io {
            operation: "inspect",
            path: absolute_path.to_path_buf(),
            source,
        }),
    }
}

fn relative_path(repository_root: &Path, absolute_path: &Path) -> Result<String, ProvenanceError> {
    let relative = absolute_path.strip_prefix(repository_root).map_err(|_| {
        ProvenanceError::PathOutsideRepository {
            path: absolute_path.to_path_buf(),
        }
    })?;
    let utf8 = relative
        .to_str()
        .ok_or_else(|| ProvenanceError::NonUtf8Path {
            path: relative.to_path_buf(),
        })?;
    Ok(utf8.replace(std::path::MAIN_SEPARATOR, "/"))
}

fn make_entry(
    relative_path: String,
    kind: SourceEntryKind,
    contents: &[u8],
) -> Result<SourceEntry, ProvenanceError> {
    let byte_length =
        u64::try_from(contents.len()).map_err(|_| ProvenanceError::LengthTooLarge {
            relative_path: relative_path.clone(),
            byte_length: contents.len(),
        })?;
    let sha256 = Sha256::digest(contents).into();
    Ok(SourceEntry {
        relative_path,
        kind,
        byte_length,
        sha256,
    })
}

fn frame_entry(
    hasher: &mut Sha256,
    relative_path: &str,
    contents: &[u8],
) -> Result<(), ProvenanceError> {
    let path_length =
        u64::try_from(relative_path.len()).map_err(|_| ProvenanceError::LengthTooLarge {
            relative_path: relative_path.to_owned(),
            byte_length: relative_path.len(),
        })?;
    let content_length =
        u64::try_from(contents.len()).map_err(|_| ProvenanceError::LengthTooLarge {
            relative_path: relative_path.to_owned(),
            byte_length: contents.len(),
        })?;
    hasher.update(path_length.to_le_bytes());
    hasher.update(relative_path.as_bytes());
    hasher.update(content_length.to_le_bytes());
    hasher.update(contents);
    Ok(())
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for &byte in bytes {
        output.push(DIGITS[usize::from(byte >> 4)] as char);
        output.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use sha2::{Digest, Sha256};

    use super::{
        MANIFEST_PATH, ProvenanceError, SOURCE_DOMAIN, SOURCE_SCHEMA, SourceEntryKind,
        UnsupportedPathKind, collect_source_provenance, collect_source_provenance_from_root,
        frame_entry,
    };

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestRepository {
        root: PathBuf,
    }

    impl TestRepository {
        fn new() -> Self {
            let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "ssfilt-provenance-{}-{sequence}",
                std::process::id()
            ));
            if root.exists() {
                fs::remove_dir_all(&root).expect("remove stale provenance test directory");
            }
            fs::create_dir_all(&root).expect("create provenance test directory");
            let repository = Self { root };
            repository.populate();
            repository
        }

        fn root(&self) -> &Path {
            &self.root
        }

        fn write(&self, relative_path: &str, contents: &[u8]) {
            let path = self.root.join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create provenance fixture parent");
            }
            fs::write(path, contents).expect("write provenance fixture");
        }

        fn populate(&self) {
            self.write("Cargo.toml", b"[package]\nname='fixture'\n");
            self.write("Cargo.lock", b"root-lock\n");
            self.write("src/z.rs", b"pub fn z() {}\n");
            self.write("src/nested/a.rs", b"pub fn a() {}\n");
            self.write("src/ignored.txt", b"not in the closure\n");
            self.write(
                "tools/forward_backward_protocol/Cargo.toml",
                b"[package]\nname='protocol'\n",
            );
            self.write(
                "tools/forward_backward_protocol/src/lib.rs",
                b"pub const VERSION: u8 = 1;\n",
            );
            self.write(
                "tools/forward_backward_oracle/Cargo.toml",
                b"[package]\nname='oracle'\n",
            );
            self.write("tools/forward_backward_oracle/Cargo.lock", b"oracle-lock\n");
            self.write(
                "tools/forward_backward_oracle/README.md",
                b"oracle readme\n",
            );
            self.write(
                "tools/forward_backward_oracle/src/lib.rs",
                b"pub fn oracle() {}\n",
            );
            self.write("docs/forward-backward.md", b"design\n");
            self.write("docs/forward-backward-fixtures.md", b"fixtures\n");
        }
    }

    impl Drop for TestRepository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn current_repository_closure_is_collectable() {
        let provenance =
            collect_source_provenance(b"test manifest\n").expect("collect repository provenance");
        assert!(
            provenance
                .entries()
                .iter()
                .any(|entry| entry.relative_path() == "src/lib.rs")
        );
        assert!(provenance.entries().iter().any(
            |entry| entry.relative_path() == "tools/forward_backward_oracle/src/provenance.rs"
        ));
    }

    #[test]
    fn inventory_is_sorted_deterministic_and_labels_manifest_synthetic() {
        let repository = TestRepository::new();
        let first = collect_source_provenance_from_root(repository.root(), b"manifest\n")
            .expect("collect first provenance");
        let second = collect_source_provenance_from_root(repository.root(), b"manifest\n")
            .expect("collect second provenance");
        assert_eq!(first, second);

        let repository_paths: Vec<_> = first
            .entries()
            .iter()
            .filter(|entry| entry.kind() == SourceEntryKind::Repository)
            .map(|entry| entry.relative_path())
            .collect();
        assert!(repository_paths.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(repository_paths.contains(&"src/nested/a.rs"));
        assert!(repository_paths.contains(&"src/z.rs"));
        assert!(!repository_paths.contains(&"src/ignored.txt"));
        assert!(!repository_paths.contains(&"tools/forward_backward_protocol/Cargo.lock"));

        let manifest = first.entries().last().expect("synthetic manifest entry");
        assert_eq!(manifest.relative_path(), MANIFEST_PATH);
        assert_eq!(manifest.kind(), SourceEntryKind::Synthetic);
        assert_eq!(manifest.byte_length(), 9);
        assert_eq!(manifest.kind().as_str(), "synthetic");
    }

    #[test]
    fn aggregate_digest_uses_exact_schema_v1_framing() {
        let repository = TestRepository::new();
        let provenance = collect_source_provenance_from_root(repository.root(), b"manifest\n")
            .expect("collect provenance");

        let mut expected = Sha256::new();
        expected.update(SOURCE_DOMAIN);
        expected.update([SOURCE_SCHEMA]);
        for entry in provenance
            .entries()
            .iter()
            .filter(|entry| entry.kind() == SourceEntryKind::Repository)
        {
            let contents = fs::read(repository.root().join(entry.relative_path()))
                .expect("read expected source entry");
            frame_entry(&mut expected, entry.relative_path(), &contents)
                .expect("frame expected source entry");
        }
        frame_entry(&mut expected, MANIFEST_PATH, b"manifest\n").expect("frame expected manifest");

        let expected_digest: [u8; 32] = expected.finalize().into();
        assert_eq!(provenance.source_digest(), &expected_digest);
    }

    #[test]
    fn framing_distinguishes_entry_order_and_path_content_boundaries() {
        fn digest(entries: &[(&str, &[u8])]) -> [u8; 32] {
            let mut hasher = Sha256::new();
            hasher.update(SOURCE_DOMAIN);
            hasher.update([SOURCE_SCHEMA]);
            for &(path, contents) in entries {
                frame_entry(&mut hasher, path, contents).expect("frame test entry");
            }
            hasher.finalize().into()
        }

        let ordered = digest(&[("a.rs", b"one"), ("b.rs", b"two")]);
        let reversed = digest(&[("b.rs", b"two"), ("a.rs", b"one")]);
        let first_boundary = digest(&[("ab", b"c")]);
        let second_boundary = digest(&[("a", b"bc")]);

        assert_ne!(ordered, reversed);
        assert_ne!(first_boundary, second_boundary);
    }

    #[test]
    fn source_digest_changes_with_source_or_manifest_bytes() {
        let repository = TestRepository::new();
        let initial = collect_source_provenance_from_root(repository.root(), b"manifest\n")
            .expect("collect initial provenance");
        let changed_manifest =
            collect_source_provenance_from_root(repository.root(), b"manifest!\n")
                .expect("collect changed-manifest provenance");
        assert_ne!(initial.source_digest(), changed_manifest.source_digest());

        repository.write("src/z.rs", b"pub fn changed() {}\n");
        let changed_source = collect_source_provenance_from_root(repository.root(), b"manifest\n")
            .expect("collect changed-source provenance");
        assert_ne!(initial.source_digest(), changed_source.source_digest());
    }

    #[test]
    fn hashes_are_lowercase_hex_and_manifest_hash_is_known() {
        let repository = TestRepository::new();
        let provenance = collect_source_provenance_from_root(repository.root(), b"abc")
            .expect("collect provenance");
        let manifest = provenance.entries().last().expect("manifest entry");
        assert_eq!(
            manifest.sha256_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let expected_manifest_digest: [u8; 32] = Sha256::digest(b"abc").into();
        assert_eq!(manifest.sha256(), &expected_manifest_digest);

        for digest in provenance
            .entries()
            .iter()
            .map(|entry| entry.sha256_hex())
            .chain(core::iter::once(provenance.source_digest_hex()))
        {
            assert_eq!(digest.len(), 64);
            assert!(
                digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            );
        }
    }

    #[test]
    fn missing_required_file_is_reported_by_relative_path() {
        let repository = TestRepository::new();
        fs::remove_file(repository.root().join("Cargo.lock")).expect("remove required fixture");
        let error = collect_source_provenance_from_root(repository.root(), b"manifest")
            .expect_err("missing required file must fail");
        assert!(matches!(
            error,
            ProvenanceError::MissingRequiredFile { relative_path }
                if relative_path == "Cargo.lock"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_in_recursive_source_tree_is_rejected() {
        use std::os::unix::fs::symlink;

        let repository = TestRepository::new();
        symlink(
            repository.root().join("src/z.rs"),
            repository.root().join("src/link.rs"),
        )
        .expect("create source symlink");
        let error = collect_source_provenance_from_root(repository.root(), b"manifest")
            .expect_err("source symlink must fail");
        assert!(matches!(
            error,
            ProvenanceError::UnsupportedPath {
                relative_path,
                kind: UnsupportedPathKind::Symlink,
            } if relative_path == "src/link.rs"
        ));
    }
}
