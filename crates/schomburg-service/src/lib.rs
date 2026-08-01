//! Portable shell-facing facade over Schomburg's existing components.

use schomburg_agent::{Agent, AgentError, UpdateRecordResult, next_eligible_run};
use schomburg_connector::{ConnectorExtension, PresentationRegistry};
use schomburg_connector_git::{GitConnectorExtension, GitPresenter};
use schomburg_presenter::Presenter;
use schomburg_store::{
    ConnectionId, ConnectionRecord, ConnectionStatus, DiscoveredSource, DiscoveredSourceId,
    GlobalMonitoringState, ReconciliationConfiguration, Store,
};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

pub struct SchomburgService {
    store: Store,
    updating: Mutex<bool>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceStatus {
    pub configuration: ReconciliationConfiguration,
    pub connected_sources: usize,
    pub awaiting_consent: usize,
    pub update_running: bool,
}
#[derive(Debug)]
pub enum ServiceError {
    Database(String),
    Agent(String),
    UpdateAlreadyRunning,
    UnknownSource,
    UnknownConnection,
    PlatformUnavailable,
}
impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(v) => write!(f, "database error: {v}"),
            Self::Agent(v) => write!(f, "service operation failed: {v}"),
            Self::UpdateAlreadyRunning => write!(f, "Update Record already running"),
            Self::UnknownSource => write!(f, "unknown source"),
            Self::UnknownConnection => write!(f, "unknown connection"),
            Self::PlatformUnavailable => write!(f, "platform operation unavailable"),
        }
    }
}
impl std::error::Error for ServiceError {}
impl SchomburgService {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ServiceError> {
        Ok(Self {
            store: Store::open(path).map_err(|e| ServiceError::Database(e.to_string()))?,
            updating: Mutex::new(false),
        })
    }
    fn agent(&self) -> Agent<'_> {
        Agent::new(
            &self.store,
            [Box::new(GitConnectorExtension) as Box<dyn ConnectorExtension>],
        )
    }
    fn presenter(&self) -> Result<Presenter, ServiceError> {
        let mut registry = PresentationRegistry::default();
        registry
            .register(Box::new(GitPresenter::new()))
            .map_err(|e| ServiceError::Agent(e.to_string()))?;
        Ok(Presenter::new(registry))
    }
    pub fn status(&self) -> Result<ServiceStatus, ServiceError> {
        let sources = self
            .store
            .list_discovered_sources()
            .map_err(|e| ServiceError::Database(e.to_string()))?;
        let connections = self
            .store
            .list_connections()
            .map_err(|e| ServiceError::Database(e.to_string()))?;
        Ok(ServiceStatus {
            configuration: self
                .store
                .reconciliation_configuration()
                .map_err(|e| ServiceError::Database(e.to_string()))?,
            connected_sources: connections
                .iter()
                .filter(|c| c.status == ConnectionStatus::Enabled)
                .count(),
            awaiting_consent: sources
                .iter()
                .filter(|s| s.status == schomburg_store::DiscoveryStatus::AwaitingDecision)
                .count(),
            update_running: *self
                .updating
                .lock()
                .map_err(|_| ServiceError::UpdateAlreadyRunning)?,
        })
    }
    pub fn update_record(&self, force: bool) -> Result<UpdateRecordResult, ServiceError> {
        let mut guard = self
            .updating
            .lock()
            .map_err(|_| ServiceError::UpdateAlreadyRunning)?;
        if *guard {
            return Err(ServiceError::UpdateAlreadyRunning);
        }
        *guard = true;
        let result = (|| {
            let presenter = self.presenter()?;
            self.agent()
                .update_record_once(&presenter, force)
                .map_err(map_agent)
        })();
        *guard = false;
        result
    }
    pub fn discover(&self, roots: &[PathBuf]) -> Result<usize, ServiceError> {
        self.agent().discover(roots).map_err(map_agent)
    }
    pub fn list_sources(&self) -> Result<Vec<DiscoveredSource>, ServiceError> {
        self.store
            .list_discovered_sources()
            .map_err(|e| ServiceError::Database(e.to_string()))
    }
    pub fn connect(&self, id: &DiscoveredSourceId) -> Result<ConnectionId, ServiceError> {
        self.agent().approve(id).map_err(map_agent)
    }
    pub fn decline(&self, id: &DiscoveredSourceId) -> Result<(), ServiceError> {
        self.agent().decline(id).map_err(map_agent)
    }
    pub fn list_connections(&self) -> Result<Vec<ConnectionRecord>, ServiceError> {
        self.store
            .list_connections()
            .map_err(|e| ServiceError::Database(e.to_string()))
    }
    pub fn set_connection_status(
        &self,
        id: &ConnectionId,
        status: ConnectionStatus,
    ) -> Result<(), ServiceError> {
        self.agent().set_status(id, status).map_err(map_agent)
    }
    /// Pauses an approved connection without revoking its consent.
    pub fn pause_connection(&self, id: &ConnectionId) -> Result<(), ServiceError> {
        self.set_connection_status(id, ConnectionStatus::Paused)
    }
    /// Re-enables a previously paused approved connection.
    pub fn resume_connection(&self, id: &ConnectionId) -> Result<(), ServiceError> {
        self.set_connection_status(id, ConnectionStatus::Enabled)
    }
    /// Stops future collection while retaining the source and preserved evidence.
    pub fn disconnect_connection(&self, id: &ConnectionId) -> Result<(), ServiceError> {
        self.set_connection_status(id, ConnectionStatus::Disconnected)
    }
    pub fn set_monitoring_enabled(&self, enabled: bool) -> Result<(), ServiceError> {
        let mut c = self
            .store
            .reconciliation_configuration()
            .map_err(|e| ServiceError::Database(e.to_string()))?;
        c.monitoring = if enabled {
            GlobalMonitoringState::Enabled
        } else {
            GlobalMonitoringState::Paused
        };
        c.next_run = next_eligible_run(&c, std::time::SystemTime::now()).map_err(map_agent)?;
        self.store
            .save_reconciliation_configuration(&c)
            .map_err(|e| ServiceError::Database(e.to_string()))
    }
    pub fn set_record_folder(&self, path: impl Into<String>) -> Result<(), ServiceError> {
        let mut c = self
            .store
            .reconciliation_configuration()
            .map_err(|e| ServiceError::Database(e.to_string()))?;
        c.record_folder = Some(path.into());
        self.store
            .save_reconciliation_configuration(&c)
            .map_err(|e| ServiceError::Database(e.to_string()))
    }
    pub fn set_schedule(
        &self,
        mut schedule: ReconciliationConfiguration,
    ) -> Result<(), ServiceError> {
        schedule.next_run =
            next_eligible_run(&schedule, std::time::SystemTime::now()).map_err(map_agent)?;
        self.store
            .save_reconciliation_configuration(&schedule)
            .map_err(|e| ServiceError::Database(e.to_string()))
    }
    pub fn get_schedule(&self) -> Result<ReconciliationConfiguration, ServiceError> {
        self.store
            .reconciliation_configuration()
            .map_err(|e| ServiceError::Database(e.to_string()))
    }
}
fn map_agent(error: AgentError) -> ServiceError {
    match error {
        AgentError::Store(schomburg_store::StoreError::MissingDiscoveredSource(_)) => {
            ServiceError::UnknownSource
        }
        AgentError::Store(schomburg_store::StoreError::MissingConnection(_)) => {
            ServiceError::UnknownConnection
        }
        error => ServiceError::Agent(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use git2::{Repository, Signature, Time};
    use schomburg_store::{LocalReconciliationTime, ReconciliationSchedule};
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static SERIAL: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryDirectory(PathBuf);

    impl TemporaryDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "schomburg-service-{label}-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temporary directory");
            Self(path)
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn database(directory: &TemporaryDirectory) -> PathBuf {
        directory.0.join("schomburg.sqlite3")
    }

    fn repository(directory: &TemporaryDirectory, name: &str) -> PathBuf {
        let path = directory.0.join(name);
        let repository = Repository::init(&path).expect("initialize repository");
        let tree_id = repository
            .treebuilder(None)
            .expect("tree builder")
            .write()
            .expect("write tree");
        let tree = repository.find_tree(tree_id).expect("tree");
        let signature = Signature::new(
            "Schomburg Test",
            "test@example.com",
            &Time::new(1_700_000_000, 0),
        )
        .expect("signature");
        repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "service test commit",
                &tree,
                &[],
            )
            .expect("commit");
        path
    }

    #[test]
    fn opens_persists_configuration_and_reports_status() {
        let directory = TemporaryDirectory::new("configuration");
        let path = database(&directory);
        let service = SchomburgService::open(&path).expect("open");
        service.set_record_folder("/tmp/records").expect("folder");
        service.set_monitoring_enabled(true).expect("monitoring");
        let mut configuration = service.get_schedule().expect("schedule");
        configuration.schedule = ReconciliationSchedule::Weekdays;
        configuration.time = LocalReconciliationTime {
            hour: 18,
            minute: 30,
        };
        service.set_schedule(configuration).expect("save schedule");
        let status = service.status().expect("status");
        assert_eq!(
            status.configuration.record_folder.as_deref(),
            Some("/tmp/records")
        );
        assert_eq!(
            status.configuration.monitoring,
            GlobalMonitoringState::Enabled
        );
        assert_eq!(status.connected_sources, 0);
        assert_eq!(status.awaiting_consent, 0);
        assert_eq!(
            status.configuration.schedule,
            ReconciliationSchedule::Weekdays
        );
        drop(service);

        let reopened = SchomburgService::open(&path).expect("reopen");
        assert_eq!(
            reopened
                .status()
                .expect("status")
                .configuration
                .record_folder
                .as_deref(),
            Some("/tmp/records")
        );
        assert_eq!(
            reopened.status().expect("status").configuration.time,
            LocalReconciliationTime {
                hour: 18,
                minute: 30
            }
        );
    }

    #[test]
    fn update_requires_record_folder_without_leaking_store_error() {
        let directory = TemporaryDirectory::new("missing-folder");
        let service = SchomburgService::open(database(&directory)).expect("open");
        assert!(
            matches!(service.update_record(true), Err(ServiceError::Agent(message)) if message.contains("no Record Folder"))
        );
    }

    #[test]
    fn discovery_connection_actions_and_unknown_ids_use_service_boundary() {
        let directory = TemporaryDirectory::new("discovery");
        let root = directory.0.join("roots");
        fs::create_dir_all(&root).expect("roots");
        let repository_directory = TemporaryDirectory(root);
        let _repository = repository(&repository_directory, "project");
        let service = SchomburgService::open(database(&directory)).expect("open");

        assert_eq!(
            service
                .discover(std::slice::from_ref(&repository_directory.0))
                .expect("discover"),
            1
        );
        let source = service.list_sources().expect("sources").remove(0);
        assert_eq!(service.status().expect("status").awaiting_consent, 1);
        let connection = service.connect(&source.id).expect("connect");
        assert_eq!(service.list_connections().expect("connections").len(), 1);
        service.pause_connection(&connection).expect("pause");
        service.resume_connection(&connection).expect("resume");
        service
            .disconnect_connection(&connection)
            .expect("disconnect");

        assert!(matches!(
            service.connect(&DiscoveredSourceId::new("missing")),
            Err(ServiceError::UnknownSource)
        ));
        assert!(matches!(
            service.pause_connection(&ConnectionId::new("missing")),
            Err(ServiceError::UnknownConnection)
        ));
    }

    #[test]
    fn update_guard_and_status_are_exposed_without_overlapping_work() {
        let directory = TemporaryDirectory::new("guard");
        let service = SchomburgService::open(database(&directory)).expect("open");
        *service.updating.lock().expect("guard") = true;
        assert!(service.status().expect("status").update_running);
        assert!(matches!(
            service.update_record(true),
            Err(ServiceError::UpdateAlreadyRunning)
        ));
        *service.updating.lock().expect("guard") = false;
        assert!(!service.status().expect("status").update_running);
    }
}
