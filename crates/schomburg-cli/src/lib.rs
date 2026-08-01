//! Minimal local command-line proof for Schomburg evidence import and display.

use schomburg_agent::Agent;
use schomburg_connector::{
    CompactPresentation, Connector, DetailedPresentation, PresentationError, PresentationField,
    PresentationRegistry,
};
use schomburg_connector_git::{GitConnector, GitConnectorExtension, GitPresenter};
use schomburg_core::{Event, EventId, MediaType};
use schomburg_engine::Engine;
use schomburg_store::{Store, StoreError};
use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

/// Executes one local Schomburg CLI command and returns factual terminal output.
pub fn execute(arguments: &[String]) -> Result<String, CliError> {
    match arguments.first().map(String::as_str) {
        Some("init") => initialize_database(&parse_options(&arguments[1..], &["db"])?),
        Some("import") => import_command(&arguments[1..]),
        Some("events") => list_events(&arguments[1..]),
        Some("event") => show_event(&arguments[1..]),
        Some("discover") => discover_command(&arguments[1..]),
        Some("sources") => sources_command(&arguments[1..]),
        Some("connect") => source_action(&arguments[1..], true),
        Some("decline") => source_action(&arguments[1..], false),
        Some("connections") => connections_command(&arguments[1..]),
        Some("pause") => {
            connection_action(&arguments[1..], schomburg_store::ConnectionStatus::Paused)
        }
        Some("resume") => {
            connection_action(&arguments[1..], schomburg_store::ConnectionStatus::Enabled)
        }
        Some("disconnect") => connection_action(
            &arguments[1..],
            schomburg_store::ConnectionStatus::Disconnected,
        ),
        Some("collect") => collect_command(&arguments[1..]),
        _ => Err(CliError::Usage(usage().to_owned())),
    }
}

fn agent(store: &Store) -> Agent<'_> {
    Agent::new(
        store,
        [Box::new(GitConnectorExtension) as Box<dyn schomburg_connector::ConnectorExtension>],
    )
}

fn discover_command(arguments: &[String]) -> Result<String, CliError> {
    let options = parse_options(arguments, &["root", "db"])?;
    let store = Store::open(required_option(&options, "db")?).map_err(CliError::Store)?;
    let count = agent(&store)
        .discover(&[PathBuf::from(required_option(&options, "root")?)])
        .map_err(CliError::Agent)?;
    Ok(format!("discovered: {count}\n"))
}

fn collect_command(arguments: &[String]) -> Result<String, CliError> {
    let options = parse_options(arguments, &["db"])?;
    let store = Store::open(required_option(&options, "db")?).map_err(CliError::Store)?;
    let report = agent(&store).collect_once().map_err(CliError::Agent)?;
    Ok(format!(
        "imported: {}\nfailed: {}\nskipped: {}\n",
        report.imported, report.failed, report.skipped
    ))
}
fn sources_command(arguments: &[String]) -> Result<String, CliError> {
    let options = parse_options(arguments, &["db"])?;
    let store = Store::open(required_option(&options, "db")?).map_err(CliError::Store)?;
    let sources = store.list_discovered_sources().map_err(CliError::Store)?;
    if sources.is_empty() {
        return Ok("no discovered sources\n".to_owned());
    }
    sources.into_iter().map(|s|Ok(format!("id: {}\nconnector: {}\nname: {}\nreference: {}\nstatus: {:?}\nfirst_discovered: {}\nlast_seen: {}\n",s.id.as_str(),s.connector_id.as_str(),s.display_name,s.local_reference.unwrap_or_else(||"none".to_owned()),s.status,format_local_time(s.first_discovered)?,format_local_time(s.last_seen)?))).collect::<Result<Vec<_>,CliError>>().map(|v|v.join("\n"))
}
fn source_action(arguments: &[String], approve: bool) -> Result<String, CliError> {
    let id = arguments
        .first()
        .ok_or_else(|| CliError::Usage(usage().to_owned()))?;
    let opts = parse_options(&arguments[1..], &["db"])?;
    let store = Store::open(required_option(&opts, "db")?).map_err(CliError::Store)?;
    if approve {
        let connection = agent(&store)
            .approve(&schomburg_store::DiscoveredSourceId::new(id.clone()))
            .map_err(CliError::Agent)?;
        Ok(format!("connected: {}\n", connection.as_str()))
    } else {
        agent(&store)
            .decline(&schomburg_store::DiscoveredSourceId::new(id.clone()))
            .map_err(CliError::Agent)?;
        Ok("declined\n".to_owned())
    }
}
fn connections_command(arguments: &[String]) -> Result<String, CliError> {
    let opts = parse_options(arguments, &["db"])?;
    let store = Store::open(required_option(&opts, "db")?).map_err(CliError::Store)?;
    let items = store.list_connections().map_err(CliError::Store)?;
    if items.is_empty() {
        return Ok("no connections\n".to_owned());
    }
    items.into_iter().map(|c| {
        Ok(format!("id: {}\nsource: {}\nconnector: {}\nstatus: {:?}\npolicy: {:?}\napproved_at: {}\nlast_attempt: {}\nlast_success: {}\nlast_error: {}\n",
            c.id.as_str(), c.source_id.as_str(), c.connector_id.as_str(), c.status, c.policy,
            format_local_time(c.approved_at)?,
            c.last_attempt.map(format_local_time).transpose()?.unwrap_or_else(|| "never".to_owned()),
            c.last_success.map(format_local_time).transpose()?.unwrap_or_else(|| "never".to_owned()),
            c.last_error.unwrap_or_else(|| "none".to_owned())))
    }).collect::<Result<Vec<_>, CliError>>().map(|v| v.join("\n"))
}
fn connection_action(
    arguments: &[String],
    status: schomburg_store::ConnectionStatus,
) -> Result<String, CliError> {
    let id = arguments
        .first()
        .ok_or_else(|| CliError::Usage(usage().to_owned()))?;
    let opts = parse_options(&arguments[1..], &["db"])?;
    let store = Store::open(required_option(&opts, "db")?).map_err(CliError::Store)?;
    agent(&store)
        .set_status(&schomburg_store::ConnectionId::new(id.clone()), status)
        .map_err(CliError::Agent)?;
    Ok(format!("{:?}\n", status).to_lowercase())
}

fn initialize_database(options: &BTreeMap<String, String>) -> Result<String, CliError> {
    let database_path = required_option(options, "db")?;
    create_database_parent(database_path)?;
    Store::open(database_path).map_err(CliError::Store)?;
    Ok(format!("database initialized: {database_path}\n"))
}

fn import_command(arguments: &[String]) -> Result<String, CliError> {
    if arguments.first().map(String::as_str) != Some("git") {
        return Err(CliError::Usage(usage().to_owned()));
    }
    let options = parse_options(&arguments[1..], &["repo", "db"])?;
    let repository_path = required_option(&options, "repo")?;
    let database_path = required_option(&options, "db")?;
    let store = Store::open(database_path).map_err(CliError::Store)?;
    let mut connector = GitConnector::open(repository_path).map_err(CliError::Git)?;
    let mut engine = Engine::new(&store);
    engine
        .register(connector.descriptor().clone())
        .map_err(CliError::Engine)?;
    engine.collect(&mut connector).map_err(CliError::Engine)?;
    let report = connector
        .last_report()
        .ok_or(CliError::MissingImportReport)?;

    Ok(format!(
        "imported: {}\nduplicates: {}\nrejected: {}\nfailed: {}\n",
        report.imported(),
        report.duplicates(),
        report.rejected(),
        report.failed()
    ))
}

fn list_events(arguments: &[String]) -> Result<String, CliError> {
    let (raw, options) = parse_presentation_options(arguments)?;
    let database_path = required_option(&options, "db")?;
    let store = Store::open(database_path).map_err(CliError::Store)?;
    let events = store.list_events().map_err(CliError::Store)?;
    if events.is_empty() {
        return Ok("no events\n".to_owned());
    }

    if raw {
        return Ok(events
            .iter()
            .map(format_raw_event_summary)
            .collect::<Vec<_>>()
            .join("\n"));
    }
    let presenters = presentation_registry()?;
    events
        .iter()
        .map(|event| {
            presenters
                .present_compact(event)
                .map_err(CliError::Presentation)
        })
        .map(|result| result.and_then(|presentation| format_compact_presentation(&presentation)))
        .collect::<Result<Vec<_>, _>>()
        .map(|items| items.join("\n"))
}

fn show_event(arguments: &[String]) -> Result<String, CliError> {
    let Some(event_id) = arguments.first() else {
        return Err(CliError::Usage(usage().to_owned()));
    };
    if event_id.is_empty() || event_id.starts_with("--") {
        return Err(CliError::InvalidEventId(event_id.clone()));
    }
    let (raw, options) = parse_presentation_options(&arguments[1..])?;
    let database_path = required_option(&options, "db")?;
    let store = Store::open(database_path).map_err(CliError::Store)?;
    let event = store
        .get_event(&EventId::new(event_id.clone()))
        .map_err(CliError::Store)?
        .ok_or_else(|| CliError::EventNotFound(event_id.clone()))?;
    if raw {
        return Ok(format_raw_event_record(&event));
    }
    let presentation = presentation_registry()?
        .present_detailed(&event)
        .map_err(CliError::Presentation)?;
    format_detailed_presentation(&presentation)
}

fn parse_presentation_options(
    arguments: &[String],
) -> Result<(bool, BTreeMap<String, String>), CliError> {
    let mut raw = false;
    let mut value_options = Vec::new();
    for argument in arguments {
        if argument == "--raw" {
            if raw {
                return Err(CliError::Usage(usage().to_owned()));
            }
            raw = true;
        } else {
            value_options.push(argument.clone());
        }
    }
    Ok((raw, parse_options(&value_options, &["db"])?))
}

fn parse_options(
    arguments: &[String],
    allowed_options: &[&str],
) -> Result<BTreeMap<String, String>, CliError> {
    let mut options = BTreeMap::new();
    let mut index = 0;
    while index < arguments.len() {
        let flag = &arguments[index];
        if !flag.starts_with("--") {
            return Err(CliError::Usage(usage().to_owned()));
        }
        let Some(value) = arguments.get(index + 1) else {
            return Err(CliError::MissingOptionValue(flag.clone()));
        };
        let name = flag.trim_start_matches("--");
        if name.is_empty()
            || !allowed_options.contains(&name)
            || options.insert(name.to_owned(), value.clone()).is_some()
        {
            return Err(CliError::Usage(usage().to_owned()));
        }
        index += 2;
    }
    Ok(options)
}

fn required_option<'a>(
    options: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, CliError> {
    options
        .get(name)
        .filter(|value| !value.is_empty())
        .map(String::as_str)
        .ok_or_else(|| CliError::MissingRequiredOption(name.to_owned()))
}

fn create_database_parent(database_path: &str) -> Result<(), CliError> {
    let path = Path::new(database_path);
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    fs::create_dir_all(parent).map_err(|error| CliError::DatabasePath {
        path: parent.to_path_buf(),
        message: error.to_string(),
    })
}

fn presentation_registry() -> Result<PresentationRegistry, CliError> {
    let mut registry = PresentationRegistry::default();
    registry
        .register(Box::new(GitPresenter::new()))
        .map_err(CliError::Presentation)?;
    Ok(registry)
}

fn format_compact_presentation(presentation: &CompactPresentation) -> Result<String, CliError> {
    let mut output = format!(
        "{}\n{}\n",
        presentation.source_label(),
        presentation.title()
    );
    if let Some(subtitle) = presentation.subtitle() {
        output.push_str(&format!("{subtitle}\n"));
    }
    append_fields(&mut output, presentation.identifiers());
    append_fields(&mut output, presentation.key_fields());
    output.push_str(&format!(
        "Occurred: {}\n",
        format_local_time(presentation.timestamp())?
    ));
    Ok(output)
}

fn format_detailed_presentation(presentation: &DetailedPresentation) -> Result<String, CliError> {
    let mut output = format!(
        "{}\n{}\n",
        presentation.source_label(),
        presentation.title()
    );
    output.push_str(&format!(
        "Occurred: {}\n",
        format_local_time(presentation.timestamp())?
    ));
    append_fields(&mut output, presentation.fields());
    if !presentation.technical_identifiers().is_empty() {
        output.push_str("Technical identifiers:\n");
        append_fields(&mut output, presentation.technical_identifiers());
    }
    Ok(output)
}

fn append_fields(output: &mut String, fields: &[PresentationField]) {
    for field in fields {
        output.push_str(field.label());
        output.push_str(": ");
        output.push_str(field.value());
        output.push('\n');
    }
}

fn format_local_time(time: SystemTime) -> Result<String, CliError> {
    let utc = OffsetDateTime::from(time);
    let offset = UtcOffset::local_offset_at(utc).map_err(|error| CliError::Timestamp {
        message: error.to_string(),
    })?;
    utc.to_offset(offset)
        .format(format_description!("[year]-[month]-[day] [hour]:[minute]:[second] [offset_hour sign:mandatory]:[offset_minute]"))
        .map_err(|error| CliError::Timestamp {
            message: error.to_string(),
        })
}

fn format_raw_event_summary(event: &Event) -> String {
    format!(
        "id: {}\noccurred_at: {}\ncaptured_at: {}\nconnector: {}\nsource: {}\nkind: {}\n",
        event.id().as_str(),
        format_system_time(event.occurred_at().as_system_time()),
        format_system_time(event.captured_at().as_system_time()),
        event.source().connector_id().as_str(),
        event.source().reference().as_str(),
        event.kind().as_str(),
    )
}

fn format_raw_event_record(event: &Event) -> String {
    let mut output = format_raw_event_summary(event);
    output.push_str(&format!(
        "schema_version: {}\n",
        event.schema_version().as_str()
    ));
    output.push_str(&format!(
        "payload_media_type: {}\n",
        event
            .payload()
            .media_type()
            .map(MediaType::as_str)
            .unwrap_or("none")
    ));
    for (key, value) in event.payload().metadata() {
        output.push_str(&format!("metadata.{}: {}\n", key.as_str(), value.as_str()));
    }
    output.push_str(&format!(
        "payload_bytes_hex: {}\n",
        hex(event.payload().bytes())
    ));
    if event.kind().as_str() == "git.commit"
        && let Ok(raw_commit) = std::str::from_utf8(event.payload().bytes())
    {
        output.push_str("git_commit_raw:\n");
        output.push_str(raw_commit);
        if !raw_commit.ends_with('\n') {
            output.push('\n');
        }
    }
    output
}

fn format_system_time(time: SystemTime) -> String {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("+{}.{:09}s", duration.as_secs(), duration.subsec_nanos()),
        Err(error) => {
            let duration = error.duration();
            format!("-{}.{:09}s", duration.as_secs(), duration.subsec_nanos())
        }
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

fn usage() -> &'static str {
    "usage:\n  schomburg init --db <database-path>\n  schomburg discover --root <path> --db <database-path>\n  schomburg sources --db <database-path>\n  schomburg connect|decline <source-id> --db <database-path>\n  schomburg connections --db <database-path>\n  schomburg pause|resume|disconnect <connection-id> --db <database-path>\n  schomburg collect --db <database-path>\n  schomburg events --db <database-path> [--raw]\n  schomburg event <event-id> --db <database-path> [--raw]\n  schomburg import git --repo <repository-path> --db <database-path>\n"
}

/// Errors returned by the local proof CLI.
#[derive(Debug)]
pub enum CliError {
    /// The command shape is not supported.
    Usage(String),
    /// An option was provided without a value.
    MissingOptionValue(String),
    /// A required explicit option was omitted.
    MissingRequiredOption(String),
    /// The supplied event ID cannot identify an event.
    InvalidEventId(String),
    /// No event exists with the requested identifier.
    EventNotFound(String),
    /// The database parent directory could not be created.
    DatabasePath { path: PathBuf, message: String },
    /// The store could not open or read the database.
    Store(StoreError),
    /// The Git connector could not open or import the repository.
    Git(schomburg_connector_git::GitConnectorError),
    /// The engine could not register or collect the connector.
    Engine(schomburg_engine::EngineError),
    /// The connector did not make an import report available.
    MissingImportReport,
    /// Agent discovery or collection failed.
    Agent(schomburg_agent::AgentError),
    /// Connector-owned presentation could not render stored factual evidence.
    Presentation(PresentationError),
    /// A stored timestamp could not be formatted in the local timezone.
    Timestamp { message: String },
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(value) => write!(formatter, "{value}"),
            Self::MissingOptionValue(option) => write!(formatter, "missing value for {option}"),
            Self::MissingRequiredOption(option) => write!(formatter, "missing required --{option}"),
            Self::InvalidEventId(id) => write!(formatter, "invalid EventId: {id:?}"),
            Self::EventNotFound(id) => write!(formatter, "event not found: {id}"),
            Self::DatabasePath { path, message } => {
                write!(
                    formatter,
                    "cannot create database directory {}: {message}",
                    path.display()
                )
            }
            Self::Store(error) => write!(formatter, "database error: {error}"),
            Self::Git(error) => write!(formatter, "Git import error: {error}"),
            Self::Engine(error) => write!(formatter, "connector error: {error}"),
            Self::MissingImportReport => write!(formatter, "Git import completed without a report"),
            Self::Agent(error) => write!(formatter, "agent error: {error}"),
            Self::Presentation(error) => write!(formatter, "presentation error: {error}"),
            Self::Timestamp { message } => {
                write!(formatter, "timestamp formatting error: {message}")
            }
        }
    }
}

impl std::error::Error for CliError {}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature, Time};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryDirectory {
        path: PathBuf,
    }

    impl TemporaryDirectory {
        fn new(label: &str) -> Self {
            let serial = DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "schomburg-cli-{label}-{}-{serial}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temporary directory");
            Self { path }
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn create_commit(repository: &Repository, message: &str) -> git2::Oid {
        let tree_id = repository
            .treebuilder(None)
            .expect("create tree builder")
            .write()
            .expect("write empty tree");
        let tree = repository.find_tree(tree_id).expect("find tree");
        let signature =
            Signature::new("Author", "author@example.com", &Time::new(100, 0)).expect("signature");
        repository
            .commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
            .expect("create commit")
    }

    #[test]
    fn init_creates_an_explicit_database_parent_and_valid_database() {
        let directory = TemporaryDirectory::new("init");
        let database = directory.path.join("nested").join("proof.sqlite3");

        let output = execute(&arguments(&[
            "init",
            "--db",
            database.to_str().expect("path"),
        ]))
        .expect("initialize database");

        assert!(database.exists());
        assert!(output.contains("database initialized:"));
        Store::open(&database).expect("reopen initialized database");
    }

    #[test]
    fn import_reimport_events_and_event_use_the_real_architecture() {
        let directory = TemporaryDirectory::new("import");
        let repository_path = directory.path.join("repository");
        let repository = Repository::init(&repository_path).expect("initialize repository");
        let commit_id = create_commit(&repository, "subject\n\nmessage café\n");
        let database = directory.path.join("database.sqlite3");
        let database_arg = database.to_str().expect("database path");
        let repository_arg = repository_path.to_str().expect("repository path");
        execute(&arguments(&["init", "--db", database_arg])).expect("initialize database");

        let first = execute(&arguments(&[
            "import",
            "git",
            "--repo",
            repository_arg,
            "--db",
            database_arg,
        ]))
        .expect("first import");
        let second = execute(&arguments(&[
            "import",
            "git",
            "--repo",
            repository_arg,
            "--db",
            database_arg,
        ]))
        .expect("second import");
        let store = Store::open(&database).expect("open database");
        let event = store
            .list_events()
            .expect("list events")
            .into_iter()
            .next()
            .expect("imported event");
        let events =
            execute(&arguments(&["events", "--db", database_arg])).expect("events command");
        let detail = execute(&arguments(&[
            "event",
            event.id().as_str(),
            "--db",
            database_arg,
        ]))
        .expect("event command");

        assert!(first.contains("imported: 1"));
        assert!(first.contains("duplicates: 0"));
        assert!(second.contains("imported: 0"));
        assert!(second.contains("duplicates: 1"));
        assert!(events.contains("Git\nsubject\n"));
        assert!(events.contains("Repository: repository"));
        assert!(events.contains(&commit_id.to_string()[..12]));
        assert!(!events.contains(event.id().as_str()));
        assert!(!events.contains("git-dir-hex:"));
        assert!(!events.contains("schema_version:"));
        assert!(!events.contains("payload_bytes_hex:"));
        assert!(detail.contains(&format!("Commit hash: {commit_id}")));
        assert!(detail.contains("message café"));
        assert!(detail.contains("Author name: Author"));
        assert!(!detail.contains("payload_bytes_hex:"));
        assert!(!detail.contains("schema_version:"));
        assert!(!detail.contains("summary:"));
        assert!(!detail.contains("context:"));

        let raw_events = execute(&arguments(&["events", "--db", database_arg, "--raw"]))
            .expect("raw events command");
        let raw_detail = execute(&arguments(&[
            "event",
            event.id().as_str(),
            "--db",
            database_arg,
            "--raw",
        ]))
        .expect("raw event command");
        assert!(raw_events.contains(event.id().as_str()));
        assert!(raw_detail.contains("payload_bytes_hex:"));
        assert!(raw_detail.contains("schema_version:"));
    }

    #[test]
    fn invalid_repository_paths_fail_clearly() {
        let directory = TemporaryDirectory::new("invalid-repository");
        let database = directory.path.join("database.sqlite3");
        let missing_repository = directory.path.join("missing");
        let database_arg = database.to_str().expect("database path");
        execute(&arguments(&["init", "--db", database_arg])).expect("initialize database");

        assert!(matches!(
            execute(&arguments(&[
                "import",
                "git",
                "--repo",
                missing_repository.to_str().expect("missing path"),
                "--db",
                database_arg,
            ])),
            Err(CliError::Git(
                schomburg_connector_git::GitConnectorError::PathDoesNotExist { .. }
            ))
        ));
        assert!(matches!(
            execute(&arguments(&[
                "import",
                "git",
                "--repo",
                directory.path.to_str().expect("directory path"),
                "--db",
                database_arg,
            ])),
            Err(CliError::Git(
                schomburg_connector_git::GitConnectorError::NotRepository { .. }
            ))
        ));
    }

    #[test]
    fn missing_or_invalid_event_ids_fail_clearly() {
        let directory = TemporaryDirectory::new("event-id");
        let database = directory.path.join("database.sqlite3");
        let database_arg = database.to_str().expect("database path");
        execute(&arguments(&["init", "--db", database_arg])).expect("initialize database");

        assert!(matches!(
            execute(&arguments(&["event", "missing", "--db", database_arg])),
            Err(CliError::EventNotFound(id)) if id == "missing"
        ));
        assert!(matches!(
            execute(&[
                "event".to_owned(),
                String::new(),
                "--db".to_owned(),
                database_arg.to_owned(),
            ]),
            Err(CliError::InvalidEventId(id)) if id.is_empty()
        ));
    }
}
