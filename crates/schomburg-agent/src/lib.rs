//! Portable run-once lifecycle for discovery, consent, and approved collection.

use schomburg_connector::{ConnectorExtension, DiscoveredSourceCandidate};
use schomburg_engine::Engine;
use schomburg_presenter::{Presenter, record_date_for};
use schomburg_store::{
    ConnectionId, ConnectionStatus, DiscoveredSource, DiscoveredSourceId, Store, StoreError,
};
use schomburg_store::{
    GlobalMonitoringState, LocalReconciliationTime, ReconciliationConfiguration,
    ReconciliationSchedule,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::PathBuf,
    time::SystemTime,
};
use time::{OffsetDateTime, UtcOffset, Weekday};

/// Calculates the next local eligible schedule occurrence after `now`.
pub fn next_eligible_run(
    configuration: &ReconciliationConfiguration,
    now: SystemTime,
) -> Result<Option<SystemTime>, AgentError> {
    if configuration.monitoring == GlobalMonitoringState::Paused {
        return Ok(None);
    };
    if configuration.time.hour > 23 || configuration.time.minute > 59 {
        return Err(AgentError::InvalidLocalTime);
    };
    let utc = OffsetDateTime::from(now);
    let offset =
        UtcOffset::local_offset_at(utc).map_err(|e| AgentError::Schedule(e.to_string()))?;
    let local = utc.to_offset(offset);
    for days in 0..8 {
        let candidate_date = local
            .date()
            .checked_add(time::Duration::days(days))
            .ok_or_else(|| AgentError::Schedule("date overflow".to_owned()))?;
        if !eligible(configuration.schedule, candidate_date.weekday()) {
            continue;
        }
        let candidate = candidate_date
            .with_hms(configuration.time.hour, configuration.time.minute, 0)
            .map_err(|e| AgentError::Schedule(e.to_string()))?
            .assume_offset(offset);
        if days > 0 || candidate >= local {
            return Ok(Some(SystemTime::from(candidate)));
        }
    }
    Ok(None)
}
fn eligible(schedule: ReconciliationSchedule, day: Weekday) -> bool {
    match schedule {
        ReconciliationSchedule::Daily => true,
        ReconciliationSchedule::Weekdays => !matches!(day, Weekday::Saturday | Weekday::Sunday),
        ReconciliationSchedule::Selected(mask) => {
            mask & (1 << (day.number_days_from_monday())) != 0
        }
    }
}
pub fn parse_local_time(value: &str) -> Result<LocalReconciliationTime, AgentError> {
    let Some((h, m)) = value.split_once(':') else {
        return Err(AgentError::InvalidLocalTime);
    };
    let Ok(hour) = h.parse() else {
        return Err(AgentError::InvalidLocalTime);
    };
    let Ok(minute) = m.parse() else {
        return Err(AgentError::InvalidLocalTime);
    };
    if hour > 23 || minute > 59 {
        return Err(AgentError::InvalidLocalTime);
    };
    Ok(LocalReconciliationTime { hour, minute })
}
pub fn parse_schedule(value: &str) -> Result<ReconciliationSchedule, AgentError> {
    match value {
        "daily" => Ok(ReconciliationSchedule::Daily),
        "weekdays" => Ok(ReconciliationSchedule::Weekdays),
        other => {
            let mut mask = 0;
            for token in other.split(',') {
                let bit = match token {
                    "mon" => 0,
                    "tue" => 1,
                    "wed" => 2,
                    "thu" => 3,
                    "fri" => 4,
                    "sat" => 5,
                    "sun" => 6,
                    _ => return Err(AgentError::InvalidWeekdays),
                };
                mask |= 1 << bit;
            }
            if mask == 0 {
                Err(AgentError::InvalidWeekdays)
            } else {
                Ok(ReconciliationSchedule::Selected(mask))
            }
        }
    }
}

/// Machine-level coordinator. It owns no source parsing; extensions do.
pub struct Agent<'a> {
    store: &'a Store,
    extensions: BTreeMap<String, Box<dyn ConnectorExtension>>,
}
impl<'a> Agent<'a> {
    pub fn new(
        store: &'a Store,
        extensions: impl IntoIterator<Item = Box<dyn ConnectorExtension>>,
    ) -> Self {
        let mut map = BTreeMap::new();
        for extension in extensions {
            map.insert(extension.connector_id().as_str().to_owned(), extension);
        }
        Self {
            store,
            extensions: map,
        }
    }
    pub fn discover(&self, roots: &[PathBuf]) -> Result<usize, AgentError> {
        let now = SystemTime::now();
        let mut count = 0;
        for extension in self.extensions.values() {
            for candidate in extension.discover(roots).map_err(AgentError::Extension)? {
                let source = to_source(candidate, now);
                self.store
                    .upsert_discovered_source(&source)
                    .map_err(AgentError::Store)?;
                count += 1;
            }
        }
        Ok(count)
    }
    pub fn approve(&self, id: &DiscoveredSourceId) -> Result<ConnectionId, AgentError> {
        let connection =
            ConnectionId::new(format!("connection:{}:{}", id.as_str(), unique_suffix()));
        self.store
            .approve_source(id, &connection, SystemTime::now())
            .map_err(AgentError::Store)?;
        Ok(connection)
    }
    pub fn decline(&self, id: &DiscoveredSourceId) -> Result<(), AgentError> {
        self.store.decline_source(id).map_err(AgentError::Store)
    }
    pub fn set_status(
        &self,
        id: &ConnectionId,
        status: ConnectionStatus,
    ) -> Result<(), AgentError> {
        self.store
            .set_connection_status(id, status, SystemTime::now())
            .map_err(AgentError::Store)
    }
    pub fn collect_once(&self) -> Result<CollectionCycleReport, AgentError> {
        let mut report = CollectionCycleReport::default();
        for connection in self.store.list_connections().map_err(AgentError::Store)? {
            if connection.status != ConnectionStatus::Enabled {
                report.skipped += 1;
                continue;
            }
            let now = SystemTime::now();
            let Some(extension) = self.extensions.get(connection.connector_id.as_str()) else {
                self.store
                    .update_connection_result(
                        &connection.id,
                        now,
                        false,
                        Some("unsupported connector"),
                    )
                    .map_err(AgentError::Store)?;
                report.failed += 1;
                continue;
            };
            let mut connector = match extension.open_connection(&connection.configuration) {
                Ok(v) => v,
                Err(e) => {
                    self.store
                        .update_connection_result(&connection.id, now, false, Some(&e.to_string()))
                        .map_err(AgentError::Store)?;
                    report.failed += 1;
                    continue;
                }
            };
            let mut engine = Engine::new(self.store);
            if let Err(error) = engine.register(extension.descriptor()) {
                self.store
                    .update_connection_result(&connection.id, now, false, Some(&error.to_string()))
                    .map_err(AgentError::Store)?;
                report.failed += 1;
                continue;
            }
            match engine.collect(connector.as_mut()) {
                Ok(r) => {
                    self.store
                        .update_connection_result(&connection.id, now, true, None)
                        .map_err(AgentError::Store)?;
                    report.imported += r.accepted_events();
                }
                Err(e) => {
                    self.store
                        .update_connection_result(&connection.id, now, false, Some(&e.to_string()))
                        .map_err(AgentError::Store)?;
                    report.failed += 1;
                }
            }
        }
        Ok(report)
    }

    /// Runs one manual Update Record operation without changing monitoring state.
    pub fn update_record_once(
        &self,
        presenter: &Presenter,
        force: bool,
    ) -> Result<UpdateRecordResult, AgentError> {
        let mut config = self
            .store
            .reconciliation_configuration()
            .map_err(AgentError::Store)?;
        if config.record_folder.is_none() {
            return Err(AgentError::NoRecordFolder);
        }
        if config.monitoring == GlobalMonitoringState::Paused && !force {
            return Err(AgentError::MonitoringPaused);
        }
        config.last_attempt = Some(SystemTime::now());
        config.state = schomburg_store::ReconciliationState::Running;
        config.last_error = None;
        self.store
            .save_reconciliation_configuration(&config)
            .map_err(AgentError::Store)?;
        let before: BTreeSet<String> = self
            .store
            .list_events()
            .map_err(AgentError::Store)?
            .into_iter()
            .map(|e| e.id().as_str().to_owned())
            .collect();
        let collection = match self.collect_once() {
            Ok(value) => value,
            Err(error) => {
                config.state = schomburg_store::ReconciliationState::Failed;
                config.last_error = Some(error.to_string());
                self.store
                    .save_reconciliation_configuration(&config)
                    .map_err(AgentError::Store)?;
                return Err(error);
            }
        };
        let folder = config.record_folder.clone().expect("configured");
        let mut result = UpdateRecordResult {
            imported: collection.imported,
            failed: collection.failed,
            ..UpdateRecordResult::default()
        };
        for event in self.store.list_events().map_err(AgentError::Store)? {
            if !before.contains(event.id().as_str()) {
                let date = match record_date_for(event.occurred_at().as_system_time()) {
                    Ok(value) => value,
                    Err(error) => {
                        let error = AgentError::Presenter(error.to_string());
                        config.state = schomburg_store::ReconciliationState::Failed;
                        config.last_error = Some(error.to_string());
                        self.store
                            .save_reconciliation_configuration(&config)
                            .map_err(AgentError::Store)?;
                        return Err(error);
                    }
                };
                let generated = match presenter.generate_date(
                    self.store,
                    std::path::Path::new(&folder),
                    date,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        let error = AgentError::Presenter(error.to_string());
                        config.state = schomburg_store::ReconciliationState::Failed;
                        config.last_error = Some(error.to_string());
                        self.store
                            .save_reconciliation_configuration(&config)
                            .map_err(AgentError::Store)?;
                        return Err(error);
                    }
                };
                result.dates_generated += generated.dates_generated;
                result.files_created += generated.files_created;
                result.files_updated += generated.files_updated;
                result.files_unchanged += generated.files_unchanged;
            }
        }
        config.last_success = Some(SystemTime::now());
        config.state = schomburg_store::ReconciliationState::Succeeded;
        config.counts = schomburg_store::ReconciliationCounts {
            imported: result.imported as u64,
            duplicates: 0,
            rejected: 0,
            failed: result.failed as u64,
        };
        self.store
            .save_reconciliation_configuration(&config)
            .map_err(AgentError::Store)?;
        Ok(result)
    }
}
fn to_source(c: DiscoveredSourceCandidate, now: SystemTime) -> DiscoveredSource {
    DiscoveredSource {
        id: DiscoveredSourceId::new(format!(
            "source:{}:{}",
            c.connector_id().as_str(),
            c.source_identity()
        )),
        connector_id: c.connector_id().clone(),
        source_identity: c.source_identity().to_owned(),
        display_name: c.display_name().to_owned(),
        local_reference: c.local_reference().map(str::to_owned),
        first_discovered: now,
        last_seen: now,
        status: schomburg_store::DiscoveryStatus::AwaitingDecision,
        metadata: c.metadata().clone(),
        configuration: c.configuration().to_owned(),
    }
}
fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
#[derive(Default, Debug, Eq, PartialEq)]
pub struct CollectionCycleReport {
    pub imported: usize,
    pub failed: usize,
    pub skipped: usize,
}
#[derive(Default, Debug, Eq, PartialEq)]
pub struct UpdateRecordResult {
    pub imported: usize,
    pub failed: usize,
    pub dates_generated: usize,
    pub files_created: usize,
    pub files_updated: usize,
    pub files_unchanged: usize,
}
#[derive(Debug)]
pub enum AgentError {
    Store(StoreError),
    Extension(schomburg_connector::ExtensionError),
    InvalidLocalTime,
    InvalidWeekdays,
    Schedule(String),
    NoRecordFolder,
    MonitoringPaused,
    Presenter(String),
}
impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(e) => write!(f, "agent storage error: {e}"),
            Self::Extension(e) => write!(f, "agent connector error: {e}"),
            Self::InvalidLocalTime => write!(f, "invalid local reconciliation time"),
            Self::InvalidWeekdays => write!(f, "invalid selected weekdays"),
            Self::Schedule(e) => write!(f, "schedule error: {e}"),
            Self::NoRecordFolder => write!(f, "no Record Folder configured"),
            Self::MonitoringPaused => write!(f, "global monitoring is paused"),
            Self::Presenter(e) => write!(f, "record generation failed: {e}"),
        }
    }
}
impl std::error::Error for AgentError {}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature, Time};
    use schomburg_connector_git::GitConnectorExtension;
    use schomburg_store::{ConnectionStatus, DiscoveryStatus, StoreError};
    use std::{
        fs,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
        time::UNIX_EPOCH,
    };

    static SERIAL: AtomicUsize = AtomicUsize::new(0);
    struct Temp {
        path: PathBuf,
    }
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "schomburg-agent-test-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("temp");
            Self { path }
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
    fn repo(root: &Path, name: &str, seconds: i64) -> Repository {
        let path = root.join(name);
        let repo = Repository::init(path).expect("repo");
        let tree = repo
            .treebuilder(None)
            .expect("tree")
            .write()
            .expect("write");
        let tree = repo.find_tree(tree).expect("tree");
        let sig = Signature::new("Test", "test@example.com", &Time::new(seconds, 0)).expect("sig");
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &format!("{name} commit"),
            &tree,
            &[],
        )
        .expect("commit");
        drop(tree);
        repo
    }
    fn agent(store: &Store) -> Agent<'_> {
        Agent::new(
            store,
            [Box::new(GitConnectorExtension) as Box<dyn ConnectorExtension>],
        )
    }
    fn record_presenter() -> Presenter {
        let mut registry = schomburg_connector::PresentationRegistry::default();
        registry
            .register(Box::new(schomburg_connector_git::GitPresenter::new()))
            .expect("presenter");
        Presenter::new(registry)
    }

    #[test]
    fn update_record_requires_folder_and_respects_pause() {
        let store = Store::open_in_memory().expect("store");
        let agent = agent(&store);
        let presenter = record_presenter();
        assert!(matches!(
            agent.update_record_once(&presenter, false),
            Err(AgentError::NoRecordFolder)
        ));
        let mut config = store.reconciliation_configuration().expect("config");
        config.record_folder = Some(
            std::env::temp_dir()
                .join("schomburg-agent-update-test")
                .to_string_lossy()
                .into_owned(),
        );
        store
            .save_reconciliation_configuration(&config)
            .expect("save");
        assert!(matches!(
            agent.update_record_once(&presenter, false),
            Err(AgentError::MonitoringPaused)
        ));
        let result = agent.update_record_once(&presenter, true).expect("forced");
        assert_eq!(result.imported, 0);
        assert_eq!(
            store
                .reconciliation_configuration()
                .expect("config")
                .monitoring,
            GlobalMonitoringState::Paused
        );
    }

    fn configuration(
        schedule: ReconciliationSchedule,
        hour: u8,
        minute: u8,
    ) -> ReconciliationConfiguration {
        ReconciliationConfiguration {
            monitoring: GlobalMonitoringState::Enabled,
            record_folder: None,
            schedule,
            time: LocalReconciliationTime { hour, minute },
            last_attempt: None,
            last_success: None,
            next_run: None,
            last_error: None,
            counts: schomburg_store::ReconciliationCounts::default(),
            state: schomburg_store::ReconciliationState::Idle,
        }
    }

    #[test]
    fn schedule_parsing_and_paused_behavior_are_explicit() {
        assert!(parse_local_time("24:00").is_err());
        assert!(parse_schedule("mon,bad").is_err());
        assert!(parse_schedule("").is_err());
        let mut paused = configuration(ReconciliationSchedule::Daily, 9, 0);
        paused.monitoring = GlobalMonitoringState::Paused;
        assert_eq!(
            next_eligible_run(&paused, UNIX_EPOCH).expect("paused"),
            None
        );
    }

    #[test]
    fn configuration_persists_without_affecting_evidence() {
        let temp = Temp::new();
        let db = temp.path.join("config.sqlite3");
        let store = Store::open(&db).expect("store");
        let mut config = store.reconciliation_configuration().expect("default");
        assert_eq!(config.monitoring, GlobalMonitoringState::Paused);
        config.monitoring = GlobalMonitoringState::Enabled;
        config.record_folder = Some("/tmp/records".to_owned());
        config.schedule = parse_schedule("mon,wed").expect("days");
        config.time = parse_local_time("09:30").expect("time");
        config.last_error = Some("failure".to_owned());
        config.counts.imported = 2;
        store
            .save_reconciliation_configuration(&config)
            .expect("save");
        drop(store);
        let reopened = Store::open(&db).expect("reopen");
        let loaded = reopened.reconciliation_configuration().expect("load");
        assert_eq!(loaded, config);
        assert!(reopened.list_events().expect("events").is_empty());
    }

    #[test]
    fn discovery_consent_collection_and_persistence_end_to_end() {
        let temp = Temp::new();
        let root = temp.path.join("roots");
        fs::create_dir_all(&root).expect("root");
        let first = repo(&root, "one", 10);
        let second = repo(&root, "two", 20);
        drop(first);
        drop(second);
        let db = temp.path.join("machine.sqlite3");
        let store = Store::open(&db).expect("store");
        let agent = agent(&store);
        assert_eq!(
            agent
                .discover(std::slice::from_ref(&root))
                .expect("discover"),
            2
        );
        assert!(store.list_events().expect("events").is_empty());
        assert_eq!(
            agent
                .discover(&[root.clone(), root.clone()])
                .expect("overlap"),
            2
        );
        let sources = store.list_discovered_sources().expect("sources");
        assert_eq!(sources.len(), 2);
        assert!(
            sources
                .iter()
                .all(|s| s.status == DiscoveryStatus::AwaitingDecision)
        );
        let first_seen = sources[0].first_discovered;
        agent.decline(&sources[0].id).expect("decline");
        agent
            .discover(std::slice::from_ref(&root))
            .expect("rediscover");
        let declined = store
            .get_discovered_source(&sources[0].id)
            .expect("get")
            .expect("source");
        assert_eq!(declined.status, DiscoveryStatus::Declined);
        assert_eq!(declined.first_discovered, first_seen);
        let approved = store
            .list_discovered_sources()
            .expect("sources")
            .into_iter()
            .find(|s| s.status == DiscoveryStatus::AwaitingDecision)
            .expect("awaiting");
        let connection = agent.approve(&approved.id).expect("approve");
        assert!(matches!(
            agent.approve(&approved.id),
            Err(AgentError::Store(StoreError::ConnectionAlreadyApproved(_)))
        ));
        let report = agent.collect_once().expect("collect");
        assert_eq!(report.imported, 1);
        assert_eq!(store.list_events().expect("events").len(), 1);
        assert_eq!(agent.collect_once().expect("repeat").imported, 0);
        assert_eq!(store.list_events().expect("events").len(), 1);
        agent
            .set_status(&connection, ConnectionStatus::Paused)
            .expect("pause");
        assert_eq!(agent.collect_once().expect("paused").skipped, 1);
        agent
            .set_status(&connection, ConnectionStatus::Enabled)
            .expect("resume");
        agent
            .set_status(&connection, ConnectionStatus::Disconnected)
            .expect("disconnect");
        assert!(matches!(
            agent.set_status(&connection, ConnectionStatus::Enabled),
            Err(AgentError::Store(
                StoreError::InvalidConnectionTransition { .. }
            ))
        ));
        drop(store);
        let reopened = Store::open(&db).expect("reopen");
        assert_eq!(
            reopened.list_discovered_sources().expect("sources").len(),
            2
        );
        assert_eq!(
            reopened.list_connections().expect("connections")[0].status,
            ConnectionStatus::Disconnected
        );
        assert_eq!(reopened.list_events().expect("events").len(), 1);
    }

    #[test]
    fn unknown_ids_and_excluded_directories_are_safe() {
        let temp = Temp::new();
        let db = temp.path.join("db.sqlite3");
        let store = Store::open(&db).expect("store");
        let agent = agent(&store);
        assert!(matches!(
            agent.approve(&DiscoveredSourceId::new("missing")),
            Err(AgentError::Store(StoreError::MissingDiscoveredSource(_)))
        ));
        assert!(matches!(
            agent.set_status(&ConnectionId::new("missing"), ConnectionStatus::Paused),
            Err(AgentError::Store(StoreError::MissingConnection(_)))
        ));
        let root = temp.path.join("root");
        fs::create_dir_all(root.join("target/repository/.git")).expect("excluded");
        assert_eq!(agent.discover(&[root]).expect("discover"), 0);
    }
}
