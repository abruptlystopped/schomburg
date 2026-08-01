//! Historical Git commit import for one local repository.
//!
//! This connector produces immutable commit events only. It has no storage
//! dependency and emits events through the engine-owned `EventSink` contract.

use git2::{ObjectType, Oid, Repository, Sort};
use schomburg_connector::{
    CompactPresentation, Connector, ConnectorCapabilities, ConnectorCapability,
    ConnectorDescriptor, ConnectorError, ConnectorExtension, DetailedPresentation,
    DiscoveredSourceCandidate, EventAcceptanceError, EventPresenter, EventSink, ExtensionError,
    PresentationError, PresentationField, RawEvidence,
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
const MAX_DISCOVERY_DEPTH: usize = 8;

/// Git's discovery and approved-connection extension for the local agent.
#[derive(Default)]
pub struct GitConnectorExtension;

impl ConnectorExtension for GitConnectorExtension {
    fn connector_id(&self) -> &ConnectorId {
        static ID: std::sync::OnceLock<ConnectorId> = std::sync::OnceLock::new();
        ID.get_or_init(|| ConnectorId::new(CONNECTOR_ID))
    }

    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor::new(
            ConnectorId::new(CONNECTOR_ID),
            ConnectorCapabilities::new([ConnectorCapability::new("import.commits")]),
        )
    }

    fn discover(
        &self,
        roots: &[PathBuf],
    ) -> Result<Vec<DiscoveredSourceCandidate>, ExtensionError> {
        let mut candidates = BTreeMap::new();
        for root in roots {
            if !root.is_dir() {
                return Err(ExtensionError::InaccessibleScanRoot {
                    path: root.clone(),
                    message: "path is not a readable directory".to_owned(),
                });
            }
            discover_root(root, 0, &mut candidates)?;
        }
        Ok(candidates.into_values().collect())
    }

    fn open_connection(&self, configuration: &str) -> Result<Box<dyn Connector>, ExtensionError> {
        GitConnector::open(configuration)
            .map(|connector| Box::new(connector) as Box<dyn Connector>)
            .map_err(|error| ExtensionError::ConnectionFailed {
                message: error.to_string(),
            })
    }
}

/// Imports commits reachable from `HEAD` in one local Git repository.
pub struct GitConnector {
    repository: Repository,
    repository_reference: String,
    repository_display_name: String,
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
        let repository_display_name = repository_display_name(&git_dir);

        Ok(Self {
            repository,
            repository_reference,
            repository_display_name,
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
            MetadataKey::new("git.repository_display_name"),
            MetadataValue::new(self.repository_display_name.clone()),
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

/// Factual Git commit presenter, independent of collection and storage.
#[derive(Clone, Debug)]
pub struct GitPresenter {
    connector_id: ConnectorId,
}

impl Default for GitPresenter {
    fn default() -> Self {
        Self::new()
    }
}

impl GitPresenter {
    /// Creates a presenter for events produced by the Git connector.
    pub fn new() -> Self {
        Self {
            connector_id: ConnectorId::new(CONNECTOR_ID),
        }
    }
}

impl EventPresenter for GitPresenter {
    fn connector_id(&self) -> &ConnectorId {
        &self.connector_id
    }

    fn present_compact(&self, event: &Event) -> Result<CompactPresentation, PresentationError> {
        let commit = parse_git_commit(event, &self.connector_id)?;
        Ok(CompactPresentation::new(
            "Git",
            commit.subject,
            Some(commit.repository_display_name.clone()),
            event.occurred_at().as_system_time(),
            vec![PresentationField::new(
                "Commit",
                short_hash(&commit.commit_hash),
            )],
            vec![PresentationField::new(
                "Repository",
                commit.repository_display_name,
            )],
        ))
    }

    fn present_detailed(&self, event: &Event) -> Result<DetailedPresentation, PresentationError> {
        let commit = parse_git_commit(event, &self.connector_id)?;
        let parents = if commit.parent_hashes.is_empty() {
            "(none)".to_owned()
        } else {
            commit.parent_hashes.join(", ")
        };
        Ok(DetailedPresentation::new(
            "Git",
            commit.subject,
            event.occurred_at().as_system_time(),
            vec![
                PresentationField::new("Repository", commit.repository_display_name),
                PresentationField::new("Commit hash", commit.commit_hash),
                PresentationField::new("Author name", commit.author.name),
                PresentationField::new("Author email", commit.author.email),
                PresentationField::new("Author timestamp", commit.author.seconds.to_string()),
                PresentationField::new("Author timezone", commit.author.timezone),
                PresentationField::new("Committer name", commit.committer.name),
                PresentationField::new("Committer email", commit.committer.email),
                PresentationField::new("Committer timestamp", commit.committer.seconds.to_string()),
                PresentationField::new("Committer timezone", commit.committer.timezone),
                PresentationField::new("Parent hashes", parents),
                PresentationField::new("Commit message", commit.message),
            ],
            vec![PresentationField::new(
                "Repository reference",
                commit.repository_reference,
            )],
            Some(RawEvidence::new(
                "Raw Git commit object",
                event
                    .payload()
                    .media_type()
                    .map(|media_type| media_type.as_str().to_owned()),
                event.payload().bytes().clone(),
            )),
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

impl EventPresenter for GitConnector {
    fn connector_id(&self) -> &ConnectorId {
        self.descriptor.id()
    }

    fn present_compact(&self, event: &Event) -> Result<CompactPresentation, PresentationError> {
        GitPresenter::new().present_compact(event)
    }

    fn present_detailed(&self, event: &Event) -> Result<DetailedPresentation, PresentationError> {
        GitPresenter::new().present_detailed(event)
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

fn repository_display_name(git_dir: &Path) -> String {
    let repository_root = git_dir
        .file_name()
        .filter(|name| *name == ".git")
        .and_then(|_| git_dir.parent())
        .unwrap_or(git_dir);
    repository_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| repository_root.to_string_lossy().into_owned())
}

fn discover_root(
    directory: &Path,
    depth: usize,
    candidates: &mut BTreeMap<String, DiscoveredSourceCandidate>,
) -> Result<(), ExtensionError> {
    if depth > MAX_DISCOVERY_DEPTH {
        return Ok(());
    }
    let entries =
        fs::read_dir(directory).map_err(|error| ExtensionError::InaccessibleScanRoot {
            path: directory.to_path_buf(),
            message: error.to_string(),
        })?;
    for entry in entries {
        let entry = entry.map_err(|error| ExtensionError::DiscoveryFailed {
            message: error.to_string(),
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| ExtensionError::DiscoveryFailed {
                message: error.to_string(),
            })?;
        if file_type.is_symlink() || !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(
            name.as_ref(),
            ".git" | "target" | "node_modules" | ".schomburg" | ".build" | "build" | "dist"
        ) {
            continue;
        }
        let git_marker = path.join(".git");
        if git_marker.exists() {
            let repository =
                Repository::open(&path).map_err(|error| ExtensionError::DiscoveryFailed {
                    message: format!("open {}: {error}", path.display()),
                })?;
            let git_dir = fs::canonicalize(repository.path()).map_err(|error| {
                ExtensionError::DiscoveryFailed {
                    message: error.to_string(),
                }
            })?;
            let identity = format!("git-dir-hex:{}", hex(&path_identity_bytes(&git_dir)));
            let mut metadata = BTreeMap::new();
            metadata.insert("git.repository_reference".to_owned(), identity.clone());
            candidates.entry(identity.clone()).or_insert_with(|| {
                DiscoveredSourceCandidate::new(
                    ConnectorId::new(CONNECTOR_ID),
                    identity,
                    repository_display_name(&git_dir),
                    Some(path.to_string_lossy().into_owned()),
                    metadata,
                    path.to_string_lossy().into_owned(),
                )
            });
            continue;
        }
        discover_root(&path, depth + 1, candidates)?;
    }
    Ok(())
}

#[derive(Debug)]
struct ParsedGitCommit {
    repository_display_name: String,
    repository_reference: String,
    commit_hash: String,
    parent_hashes: Vec<String>,
    author: GitIdentity,
    committer: GitIdentity,
    subject: String,
    message: String,
}

#[derive(Debug)]
struct GitIdentity {
    name: String,
    email: String,
    seconds: i64,
    timezone: String,
}

fn parse_git_commit(
    event: &Event,
    connector_id: &ConnectorId,
) -> Result<ParsedGitCommit, PresentationError> {
    let actual_connector_id = event.source().connector_id();
    if actual_connector_id != connector_id {
        return Err(PresentationError::ConnectorProvenanceMismatch {
            expected: connector_id.clone(),
            actual: actual_connector_id.clone(),
        });
    }
    if event.kind().as_str() != EVENT_KIND {
        return Err(PresentationError::UnsupportedEventKind(
            event.kind().clone(),
        ));
    }

    let metadata = event.payload().metadata();
    let repository_display_name = required_metadata(metadata, "git.repository_display_name")?;
    let repository_reference = required_metadata(metadata, "git.repository_reference")?;
    let commit_hash = required_metadata(metadata, "git.commit_hash")?;
    let raw = std::str::from_utf8(event.payload().bytes()).map_err(|_| {
        PresentationError::MalformedPayload {
            detail: "raw Git commit object is not UTF-8".to_owned(),
        }
    })?;
    let (headers, message) =
        raw.split_once("\n\n")
            .ok_or_else(|| PresentationError::MalformedPayload {
                detail: "raw Git commit object has no header separator".to_owned(),
            })?;

    let mut author = None;
    let mut committer = None;
    let mut parent_hashes = Vec::new();
    for header in headers.lines() {
        if let Some(value) = header.strip_prefix("parent ") {
            parent_hashes.push(value.to_owned());
        } else if let Some(value) = header.strip_prefix("author ") {
            author = Some(parse_identity(value, "author")?);
        } else if let Some(value) = header.strip_prefix("committer ") {
            committer = Some(parse_identity(value, "committer")?);
        }
    }
    let subject = message.lines().next().unwrap_or_default().to_owned();

    Ok(ParsedGitCommit {
        repository_display_name,
        repository_reference,
        commit_hash,
        parent_hashes,
        author: author.ok_or(PresentationError::MissingFactualField {
            field: "raw Git commit author",
        })?,
        committer: committer.ok_or(PresentationError::MissingFactualField {
            field: "raw Git commit committer",
        })?,
        subject,
        message: message.to_owned(),
    })
}

fn required_metadata(
    metadata: &BTreeMap<MetadataKey, MetadataValue>,
    key: &'static str,
) -> Result<String, PresentationError> {
    metadata
        .get(&MetadataKey::new(key))
        .map(|value| value.as_str().to_owned())
        .ok_or(PresentationError::MissingFactualField { field: key })
}

fn parse_identity(value: &str, field: &'static str) -> Result<GitIdentity, PresentationError> {
    let (name, suffix) =
        value
            .rsplit_once(" <")
            .ok_or_else(|| PresentationError::MalformedPayload {
                detail: format!("raw Git commit {field} is missing a name or email"),
            })?;
    let (email, timestamp) =
        suffix
            .split_once("> ")
            .ok_or_else(|| PresentationError::MalformedPayload {
                detail: format!("raw Git commit {field} is missing timestamp data"),
            })?;
    let mut parts = timestamp.split_whitespace();
    let seconds = parts
        .next()
        .ok_or(PresentationError::MissingFactualField {
            field: "raw Git commit timestamp",
        })?
        .parse::<i64>()
        .map_err(|_| PresentationError::InvalidTimestamp {
            detail: format!("raw Git commit {field} timestamp is not an integer"),
        })?;
    let timezone = parts.next().ok_or(PresentationError::MissingFactualField {
        field: "raw Git commit timezone",
    })?;
    if parts.next().is_some() {
        return Err(PresentationError::MalformedPayload {
            detail: format!("raw Git commit {field} has trailing timestamp data"),
        });
    }
    if system_time_from_git_seconds(seconds).is_none() {
        return Err(PresentationError::InvalidTimestamp {
            detail: format!("raw Git commit {field} timestamp is outside SystemTime range"),
        });
    }
    Ok(GitIdentity {
        name: name.to_owned(),
        email: email.to_owned(),
        seconds,
        timezone: timezone.to_owned(),
    })
}

fn short_hash(hash: &str) -> String {
    hash.chars().take(12).collect()
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

    #[test]
    fn presenter_returns_factual_compact_and_detailed_commit_data() {
        let repository = TemporaryRepository::new();
        let message = "subject café\n\nfull message\nwith another line\n";
        let commit_id = commit(&repository.repository, message, 30, 40, &[]);
        let mut connector = GitConnector::open(&repository.path).expect("open connector");
        let mut sink = RecordingSink { events: Vec::new() };
        connector.import(&mut sink).expect("import commit");
        let event = &sink.events[0];
        let presenter = GitPresenter::new();

        let compact = presenter
            .present_compact(event)
            .expect("compact presentation");
        let detailed = presenter
            .present_detailed(event)
            .expect("detailed presentation");

        assert_eq!(compact.source_label(), "Git");
        assert_eq!(compact.title(), "subject café");
        assert!(compact.subtitle().is_some_and(|name| !name.is_empty()));
        assert_eq!(
            compact.identifiers()[0].value(),
            &commit_id.to_string()[..12]
        );
        assert!(
            detailed
                .fields()
                .iter()
                .any(|field| field.label() == "Commit message" && field.value() == message)
        );
        assert!(
            detailed
                .fields()
                .iter()
                .any(|field| field.label() == "Commit hash"
                    && field.value() == commit_id.to_string())
        );
        assert_eq!(
            detailed.raw_evidence().expect("raw evidence").bytes(),
            event.payload().bytes()
        );
    }

    #[test]
    fn presenter_rejects_unowned_kind_and_malformed_payload() {
        let repository = TemporaryRepository::new();
        commit(&repository.repository, "subject", 10, 20, &[]);
        let mut connector = GitConnector::open(&repository.path).expect("open connector");
        let mut sink = RecordingSink { events: Vec::new() };
        connector.import(&mut sink).expect("import commit");
        let event = &sink.events[0];
        let presenter = GitPresenter::new();

        let wrong_connector = Event::new(
            event.id().clone(),
            event.occurred_at().clone(),
            Source::new(
                ConnectorId::new("other.connector"),
                event.source().reference().clone(),
            ),
            event.kind().clone(),
            event.payload().clone(),
            event.captured_at().clone(),
            event.schema_version().clone(),
        );
        assert!(matches!(
            presenter.present_compact(&wrong_connector),
            Err(PresentationError::ConnectorProvenanceMismatch { .. })
        ));

        let wrong_kind = Event::new(
            event.id().clone(),
            event.occurred_at().clone(),
            event.source().clone(),
            EventKind::new("other.event"),
            event.payload().clone(),
            event.captured_at().clone(),
            event.schema_version().clone(),
        );
        assert!(matches!(
            presenter.present_compact(&wrong_kind),
            Err(PresentationError::UnsupportedEventKind(_))
        ));

        let malformed = Event::new(
            event.id().clone(),
            event.occurred_at().clone(),
            event.source().clone(),
            event.kind().clone(),
            EventPayload::new(Arc::from(b"not a commit".to_vec()), None, BTreeMap::new()),
            event.captured_at().clone(),
            event.schema_version().clone(),
        );
        assert!(matches!(
            presenter.present_compact(&malformed),
            Err(PresentationError::MissingFactualField { .. })
        ));
    }
}
