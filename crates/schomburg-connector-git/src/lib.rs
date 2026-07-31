//! Historical Git commit import for one local repository.
//!
//! This connector produces immutable commit events only. It has no storage
//! dependency and emits events through the engine-owned `EventSink` contract.

use git2::{ObjectType, Oid, Repository, Sort};
use schomburg_connector::{
    Connector, ConnectorCapabilities, ConnectorCapability, ConnectorDescriptor, ConnectorError,
    EventAcceptanceError, EventSink,
};
use schomburg_core::{
    CaptureTimestamp, ConnectorId, Event, EventId, EventKind, EventPayload, EventTimestamp,
    MediaType, MetadataKey, MetadataValue, SchemaVersion, Source, SourceReference,
};
use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const CONNECTOR_ID: &str = "schomburg.git";
const EVENT_KIND: &str = "git.commit";
const EVENT_SCHEMA_VERSION: &str = "1";

/// Imports commits reachable from `HEAD` in one local Git repository.
pub struct GitConnector {
    repository: Repository,
    repository_reference: String,
    descriptor: ConnectorDescriptor,
    last_report: Option<GitImportReport>,
}

impl GitConnector {
    /// Opens a local Git repository rooted at or above the supplied path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, GitConnectorError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(GitConnectorError::PathDoesNotExist {
                path: path.to_path_buf(),
            });
        }
        let canonical_input = fs::canonicalize(path).map_err(|error| GitConnectorError::Git {
            operation: "canonicalize repository path",
            message: error.to_string(),
        })?;
        let repository = Repository::open(&canonical_input).map_err(|error| {
            if error.code() == git2::ErrorCode::NotFound {
                GitConnectorError::NotRepository {
                    path: canonical_input.clone(),
                }
            } else {
                GitConnectorError::Git {
                    operation: "open repository",
                    message: error.to_string(),
                }
            }
        })?;
        let git_dir =
            fs::canonicalize(repository.path()).map_err(|error| GitConnectorError::Git {
                operation: "canonicalize Git directory",
                message: error.to_string(),
            })?;
        let repository_reference = format!("git-dir-hex:{}", hex(&path_identity_bytes(&git_dir)));

        Ok(Self {
            repository,
            repository_reference,
            descriptor: ConnectorDescriptor::new(
                ConnectorId::new(CONNECTOR_ID),
                ConnectorCapabilities::new([ConnectorCapability::new("import.commits")]),
            ),
            last_report: None,
        })
    }

    /// Returns the stable canonical Git-directory reference for this import.
    pub fn repository_reference(&self) -> &str {
        &self.repository_reference
    }

    /// Returns the report from the most recent import attempt, including a
    /// partial report when an import failed after processing commits.
    pub fn last_report(&self) -> Option<GitImportReport> {
        self.last_report
    }

    /// Imports commits reachable from `HEAD` through the supplied event sink.
    pub fn import(
        &mut self,
        sink: &mut dyn EventSink,
    ) -> Result<GitImportReport, GitConnectorError> {
        let mut report = GitImportReport::default();
        let result = self.import_into(sink, &mut report);
        self.last_report = Some(report);
        result.map(|()| report)
    }

    fn import_into(
        &self,
        sink: &mut dyn EventSink,
        report: &mut GitImportReport,
    ) -> Result<(), GitConnectorError> {
        if self
            .repository
            .is_empty()
            .map_err(|error| GitConnectorError::Git {
                operation: "check whether repository is empty",
                message: error.to_string(),
            })?
        {
            return Ok(());
        }

        let mut walk = self
            .repository
            .revwalk()
            .map_err(|error| GitConnectorError::Git {
                operation: "create commit walk",
                message: error.to_string(),
            })?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)
            .map_err(|error| GitConnectorError::Git {
                operation: "configure commit walk",
                message: error.to_string(),
            })?;
        walk.push_head().map_err(|error| GitConnectorError::Git {
            operation: "select HEAD commit history",
            message: error.to_string(),
        })?;

        for next in walk {
            let oid = next.map_err(|error| GitConnectorError::Git {
                operation: "walk commit history",
                message: error.to_string(),
            })?;
            let event = match self.event_for_commit(oid) {
                Ok(event) => event,
                Err(error) => {
                    report.failed += 1;
                    return Err(error);
                }
            };
            match sink.accept(event) {
                Ok(()) => report.imported += 1,
                Err(EventAcceptanceError::DuplicateEventId(_)) => report.duplicates += 1,
                Err(error) => {
                    report.rejected += 1;
                    return Err(GitConnectorError::EventAcceptance(error));
                }
            }
        }
        Ok(())
    }

    fn event_for_commit(&self, oid: Oid) -> Result<Event, GitConnectorError> {
        let commit = self
            .repository
            .find_commit(oid)
            .map_err(|error| GitConnectorError::Git {
                operation: "read commit",
                message: error.to_string(),
            })?;
        let odb = self
            .repository
            .odb()
            .map_err(|error| GitConnectorError::Git {
                operation: "open object database",
                message: error.to_string(),
            })?;
        let raw = odb.read(oid).map_err(|error| GitConnectorError::Git {
            operation: "read raw commit object",
            message: error.to_string(),
        })?;
        if raw.kind() != ObjectType::Commit {
            return Err(GitConnectorError::MalformedCommit {
                commit_hash: oid.to_string(),
                detail: "object is not a commit".to_owned(),
            });
        }
        let occurred_at = system_time_from_git_seconds(commit.committer().when().seconds())
            .ok_or_else(|| GitConnectorError::MalformedCommit {
                commit_hash: oid.to_string(),
                detail: "committer timestamp is outside the local SystemTime range".to_owned(),
            })?;

        let mut metadata = BTreeMap::new();
        metadata.insert(
            MetadataKey::new("git.repository_reference"),
            MetadataValue::new(self.repository_reference.clone()),
        );
        metadata.insert(
            MetadataKey::new("git.commit_hash"),
            MetadataValue::new(oid.to_string()),
        );
        metadata.insert(
            MetadataKey::new("git.payload_format"),
            MetadataValue::new("raw-commit-object"),
        );

        let commit_hash = oid.to_string();
        Ok(Event::new(
            EventId::new(format!(
                "git:commit:{}:{commit_hash}",
                self.repository_reference
            )),
            EventTimestamp::new(occurred_at),
            Source::new(
                ConnectorId::new(CONNECTOR_ID),
                SourceReference::new(commit_hash),
            ),
            EventKind::new(EVENT_KIND),
            EventPayload::new(
                Arc::from(raw.data().to_vec()),
                Some(MediaType::new("application/vnd.git.commit")),
                metadata,
            ),
            CaptureTimestamp::new(SystemTime::now()),
            SchemaVersion::new(EVENT_SCHEMA_VERSION),
        ))
    }
}

impl Connector for GitConnector {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    fn collect(&mut self, sink: &mut dyn EventSink) -> Result<(), ConnectorError> {
        self.import(sink)
            .map(|_| ())
            .map_err(|error| ConnectorError::collection_failed(error.to_string()))
    }
}

/// Counts from one Git import attempt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GitImportReport {
    imported: usize,
    duplicates: usize,
    rejected: usize,
    failed: usize,
}

impl GitImportReport {
    /// Returns the number of events accepted by the sink.
    pub fn imported(&self) -> usize {
        self.imported
    }

    /// Returns the number of events already preserved by the sink.
    pub fn duplicates(&self) -> usize {
        self.duplicates
    }

    /// Returns the number of events rejected by the sink.
    pub fn rejected(&self) -> usize {
        self.rejected
    }

    /// Returns the number of commit-processing failures.
    pub fn failed(&self) -> usize {
        self.failed
    }
}

/// Errors from local Git repository opening or commit import.
#[derive(Debug)]
pub enum GitConnectorError {
    /// The repository path does not exist.
    PathDoesNotExist { path: PathBuf },
    /// The path exists but does not resolve to a Git repository.
    NotRepository { path: PathBuf },
    /// libgit2 or local filesystem work failed.
    Git {
        operation: &'static str,
        message: String,
    },
    /// A commit could not be represented as factual evidence.
    MalformedCommit { commit_hash: String, detail: String },
    /// The engine-owned sink rejected a produced event.
    EventAcceptance(EventAcceptanceError),
}

impl fmt::Display for GitConnectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathDoesNotExist { path } => write!(
                formatter,
                "repository path does not exist: {}",
                path.display()
            ),
            Self::NotRepository { path } => {
                write!(formatter, "not a Git repository: {}", path.display())
            }
            Self::Git { operation, message } => {
                write!(formatter, "Git failure during {operation}: {message}")
            }
            Self::MalformedCommit {
                commit_hash,
                detail,
            } => write!(formatter, "malformed commit {commit_hash}: {detail}"),
            Self::EventAcceptance(error) => write!(formatter, "event acceptance failed: {error}"),
        }
    }
}

impl std::error::Error for GitConnectorError {}

fn system_time_from_git_seconds(seconds: i64) -> Option<SystemTime> {
    let duration = Duration::from_secs(seconds.unsigned_abs());
    if seconds >= 0 {
        UNIX_EPOCH.checked_add(duration)
    } else {
        UNIX_EPOCH.checked_sub(duration)
    }
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

#[cfg(unix)]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Signature, Time};
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static REPOSITORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryRepository {
        path: PathBuf,
        repository: Repository,
    }

    impl TemporaryRepository {
        fn new() -> Self {
            let serial = REPOSITORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "schomburg-git-connector-test-{}-{serial}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            let repository = Repository::init(&path).expect("initialize temporary repository");
            Self { path, repository }
        }
    }

    impl Drop for TemporaryRepository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    struct RecordingSink {
        events: Vec<Event>,
    }

    impl EventSink for RecordingSink {
        fn accept(&mut self, event: Event) -> Result<(), EventAcceptanceError> {
            self.events.push(event);
            Ok(())
        }
    }

    fn commit(
        repository: &Repository,
        message: &str,
        author_seconds: i64,
        committer_seconds: i64,
        parents: &[Oid],
    ) -> Oid {
        let tree_id = repository
            .treebuilder(None)
            .expect("create tree builder")
            .write()
            .expect("write empty tree");
        let tree = repository.find_tree(tree_id).expect("find tree");
        let author = Signature::new(
            "Author Name",
            "author@example.com",
            &Time::new(author_seconds, 0),
        )
        .expect("author signature");
        let committer = Signature::new(
            "Committer Name",
            "committer@example.com",
            &Time::new(committer_seconds, 60),
        )
        .expect("committer signature");
        let parent_commits: Vec<_> = parents
            .iter()
            .map(|parent| repository.find_commit(*parent).expect("find parent"))
            .collect();
        let parent_refs: Vec<_> = parent_commits.iter().collect();
        repository
            .commit(
                Some("HEAD"),
                &author,
                &committer,
                message,
                &tree,
                &parent_refs,
            )
            .expect("create commit")
    }

    #[test]
    fn valid_repository_imports_commits_in_deterministic_order() {
        let repository = TemporaryRepository::new();
        let first = commit(&repository.repository, "first", 10, 20, &[]);
        let second = commit(&repository.repository, "second", 30, 40, &[first]);
        let mut connector = GitConnector::open(&repository.path).expect("open connector");
        let mut sink = RecordingSink { events: Vec::new() };

        let report = connector.import(&mut sink).expect("import commits");

        assert_eq!(
            report,
            GitImportReport {
                imported: 2,
                ..GitImportReport::default()
            }
        );
        assert_eq!(sink.events.len(), 2);
        assert_eq!(
            sink.events[0].source().reference().as_str(),
            first.to_string()
        );
        assert_eq!(
            sink.events[1].source().reference().as_str(),
            second.to_string()
        );
    }

    #[test]
    fn event_preserves_git_fields_and_uses_committer_time() {
        let repository = TemporaryRepository::new();
        let parent = commit(&repository.repository, "parent", 10, 20, &[]);
        let message = "subject\n\nmessage body\nwith non-ASCII: café\n";
        let commit_id = commit(&repository.repository, message, 30, 40, &[parent]);
        let expected_raw = repository
            .repository
            .odb()
            .expect("object database")
            .read(commit_id)
            .expect("raw commit")
            .data()
            .to_vec();
        let mut connector = GitConnector::open(&repository.path).expect("open connector");
        let mut sink = RecordingSink { events: Vec::new() };
        let before_import = SystemTime::now();

        connector.import(&mut sink).expect("import commits");

        let event = sink
            .events
            .iter()
            .find(|event| event.source().reference().as_str() == commit_id.to_string())
            .expect("imported commit event");
        let raw = event.payload().bytes();
        let raw_text = String::from_utf8_lossy(raw);
        assert_eq!(raw.as_ref(), expected_raw.as_slice());
        assert!(raw_text.contains(&format!("parent {parent}")));
        assert!(raw_text.contains("author Author Name <author@example.com> 30 +0000"));
        assert!(raw_text.contains("committer Committer Name <committer@example.com> 40 +0100"));
        assert!(raw_text.contains(message));
        assert_eq!(
            event.occurred_at().as_system_time(),
            UNIX_EPOCH
                .checked_add(Duration::from_secs(40))
                .expect("timestamp")
        );
        assert!(event.captured_at().as_system_time() >= before_import);
        assert!(event.captured_at().as_system_time() <= SystemTime::now());
        assert_eq!(event.kind().as_str(), EVENT_KIND);
        assert_eq!(event.source().connector_id(), connector.descriptor().id());
        assert_eq!(
            event
                .payload()
                .metadata()
                .get(&MetadataKey::new("git.repository_reference"))
                .expect("repository reference")
                .as_str(),
            connector.repository_reference()
        );
    }

    #[test]
    fn same_commit_hash_in_distinct_repository_identities_has_distinct_event_ids() {
        let first_repository = TemporaryRepository::new();
        let second_repository = TemporaryRepository::new();
        let first_commit = commit(&first_repository.repository, "same", 10, 20, &[]);
        let second_commit = commit(&second_repository.repository, "same", 10, 20, &[]);
        assert_eq!(first_commit, second_commit);
        let mut first_connector =
            GitConnector::open(&first_repository.path).expect("first connector");
        let mut second_connector =
            GitConnector::open(&second_repository.path).expect("second connector");
        let mut first_sink = RecordingSink { events: Vec::new() };
        let mut second_sink = RecordingSink { events: Vec::new() };

        first_connector
            .import(&mut first_sink)
            .expect("first import");
        second_connector
            .import(&mut second_sink)
            .expect("second import");

        assert_ne!(
            first_connector.repository_reference(),
            second_connector.repository_reference()
        );
        assert_ne!(first_sink.events[0].id(), second_sink.events[0].id());
    }

    #[test]
    fn invalid_and_non_repository_paths_return_explicit_errors() {
        let directory = TemporaryRepository::new();
        let missing = directory.path.join("does-not-exist");
        assert!(matches!(
            GitConnector::open(&missing),
            Err(GitConnectorError::PathDoesNotExist { .. })
        ));

        let non_repository = std::env::temp_dir().join(format!(
            "schomburg-git-connector-not-repository-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&non_repository);
        fs::create_dir_all(&non_repository).expect("create directory");
        assert!(matches!(
            GitConnector::open(&non_repository),
            Err(GitConnectorError::NotRepository { .. })
        ));
        let _ = fs::remove_dir_all(&non_repository);
    }

    #[test]
    fn connector_produces_events_only_without_organizational_assignments() {
        let repository = TemporaryRepository::new();
        commit(&repository.repository, "commit", 10, 20, &[]);
        let mut connector = GitConnector::open(&repository.path).expect("open connector");
        let mut sink = RecordingSink { events: Vec::new() };

        connector.import(&mut sink).expect("import commits");

        assert_eq!(sink.events.len(), 1);
        assert_eq!(
            sink.events[0].source().connector_id().as_str(),
            CONNECTOR_ID
        );
    }
}
