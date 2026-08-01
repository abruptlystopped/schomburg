//! Portable shell-facing facade over Schomburg's existing components.

use schomburg_agent::{
    Agent, AgentError, UpdateRecordResult, UpdateRecordRunKind, next_eligible_run,
};
use schomburg_connector::{ConnectorExtension, PresentationRegistry};
use schomburg_connector_git::{GitConnectorExtension, GitPresenter};
use schomburg_presenter::{Presenter, record_date_for};
use schomburg_store::{
    ConnectionId, ConnectionRecord, ConnectionStatus, DiscoveredSource, DiscoveredSourceId,
    GlobalMonitoringState, ManualUpdateStatus, ReconciliationConfiguration,
    ScheduledReconciliationStatus, Store,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

pub struct SchomburgService {
    store: Store,
    database_path: PathBuf,
    updating: Arc<Mutex<bool>>,
    scheduler: Arc<(Mutex<SchedulerControl>, Condvar)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchedulerLifecycle {
    Stopped,
    Paused,
    Waiting,
    Updating,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedulerStatus {
    pub running: bool,
    pub lifecycle: SchedulerLifecycle,
    pub monitoring: GlobalMonitoringState,
    pub next_run: Option<SystemTime>,
    pub last_attempt: Option<SystemTime>,
    pub last_success: Option<SystemTime>,
    pub last_error: Option<String>,
    pub record_folder: Option<String>,
    pub connected_sources: usize,
    pub update_running: bool,
}

struct SchedulerControl {
    running: bool,
    stop_requested: bool,
    lifecycle: SchedulerLifecycle,
    handle: Option<JoinHandle<()>>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceStatus {
    pub configuration: ReconciliationConfiguration,
    pub manual_update: ManualUpdateStatus,
    pub scheduled_reconciliation: ScheduledReconciliationStatus,
    pub connected_sources: usize,
    pub awaiting_consent: usize,
    pub update_running: bool,
}
#[derive(Debug)]
pub enum ServiceError {
    Database(String),
    Agent(String),
    UpdateAlreadyRunning,
    SchedulerAlreadyRunning,
    SchedulerNotRunning,
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
            Self::SchedulerAlreadyRunning => write!(f, "scheduler already running"),
            Self::SchedulerNotRunning => write!(f, "scheduler is not running"),
            Self::UnknownSource => write!(f, "unknown source"),
            Self::UnknownConnection => write!(f, "unknown connection"),
            Self::PlatformUnavailable => write!(f, "platform operation unavailable"),
        }
    }
}
impl std::error::Error for ServiceError {}
impl SchomburgService {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ServiceError> {
        Self::open_with_scheduler(
            path.as_ref(),
            Arc::new((
                Mutex::new(SchedulerControl {
                    running: false,
                    stop_requested: false,
                    lifecycle: SchedulerLifecycle::Stopped,
                    handle: None,
                }),
                Condvar::new(),
            )),
            Arc::new(Mutex::new(false)),
        )
    }

    fn open_with_scheduler(
        path: &Path,
        scheduler: Arc<(Mutex<SchedulerControl>, Condvar)>,
        updating: Arc<Mutex<bool>>,
    ) -> Result<Self, ServiceError> {
        Ok(Self {
            store: Store::open(path).map_err(|e| ServiceError::Database(e.to_string()))?,
            database_path: path.to_path_buf(),
            updating,
            scheduler,
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
            manual_update: self
                .store
                .manual_update_status()
                .map_err(|e| ServiceError::Database(e.to_string()))?,
            scheduled_reconciliation: self
                .store
                .scheduled_reconciliation_status()
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

    /// Starts one portable scheduler loop for this service instance.
    pub fn start_scheduler(&self) -> Result<(), ServiceError> {
        let (lock, changed) = &*self.scheduler;
        let mut control = lock
            .lock()
            .map_err(|_| ServiceError::SchedulerAlreadyRunning)?;
        if control.running {
            return Err(ServiceError::SchedulerAlreadyRunning);
        }
        control.running = true;
        control.stop_requested = false;
        control.lifecycle = SchedulerLifecycle::Waiting;
        let database_path = self.database_path.clone();
        let scheduler = Arc::clone(&self.scheduler);
        let updating = Arc::clone(&self.updating);
        control.handle = Some(thread::spawn(move || {
            let Ok(service) = SchomburgService::open_with_scheduler(
                &database_path,
                Arc::clone(&scheduler),
                updating,
            ) else {
                set_scheduler_lifecycle(&scheduler, SchedulerLifecycle::Failed);
                return;
            };
            scheduler_loop(&service, &scheduler);
        }));
        changed.notify_all();
        Ok(())
    }

    /// Requests scheduler shutdown and waits for the local loop to finish.
    pub fn stop_scheduler(&self) -> Result<(), ServiceError> {
        let handle = {
            let (lock, changed) = &*self.scheduler;
            let mut control = lock.lock().map_err(|_| ServiceError::SchedulerNotRunning)?;
            if !control.running {
                return Err(ServiceError::SchedulerNotRunning);
            }
            control.stop_requested = true;
            changed.notify_all();
            control.handle.take()
        };
        if let Some(handle) = handle {
            let _ = handle.join();
        }
        Ok(())
    }

    pub fn scheduler_status(&self) -> Result<SchedulerStatus, ServiceError> {
        let status = self.status()?;
        let (lock, _) = &*self.scheduler;
        let control = lock.lock().map_err(|_| ServiceError::SchedulerNotRunning)?;
        Ok(SchedulerStatus {
            running: control.running,
            lifecycle: control.lifecycle,
            monitoring: status.configuration.monitoring,
            next_run: status
                .scheduled_reconciliation
                .next_scheduled_run
                .or(status.configuration.next_run),
            last_attempt: status.scheduled_reconciliation.last_attempt,
            last_success: status.scheduled_reconciliation.last_success,
            last_error: status.scheduled_reconciliation.last_error,
            record_folder: status.configuration.record_folder,
            connected_sources: status.connected_sources,
            update_running: status.update_running,
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
    fn scheduled_update(&self, eligible_date: String) -> Result<UpdateRecordResult, ServiceError> {
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
                .execute_update_record_once(
                    &presenter,
                    false,
                    UpdateRecordRunKind::ScheduledDailyReconciliation {
                        eligible_local_date: eligible_date,
                    },
                )
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
            .map_err(|e| ServiceError::Database(e.to_string()))?;
        self.notify_scheduler();
        Ok(())
    }
    pub fn set_record_folder(&self, path: impl Into<String>) -> Result<(), ServiceError> {
        let mut c = self
            .store
            .reconciliation_configuration()
            .map_err(|e| ServiceError::Database(e.to_string()))?;
        c.record_folder = Some(path.into());
        self.store
            .save_reconciliation_configuration(&c)
            .map_err(|e| ServiceError::Database(e.to_string()))?;
        self.notify_scheduler();
        Ok(())
    }
    pub fn set_schedule(
        &self,
        mut schedule: ReconciliationConfiguration,
    ) -> Result<(), ServiceError> {
        schedule.next_run =
            next_eligible_run(&schedule, std::time::SystemTime::now()).map_err(map_agent)?;
        self.store
            .save_reconciliation_configuration(&schedule)
            .map_err(|e| ServiceError::Database(e.to_string()))?;
        self.notify_scheduler();
        Ok(())
    }
    pub fn get_schedule(&self) -> Result<ReconciliationConfiguration, ServiceError> {
        self.store
            .reconciliation_configuration()
            .map_err(|e| ServiceError::Database(e.to_string()))
    }

    fn notify_scheduler(&self) {
        self.scheduler.1.notify_all();
    }
}

fn set_scheduler_lifecycle(
    scheduler: &Arc<(Mutex<SchedulerControl>, Condvar)>,
    lifecycle: SchedulerLifecycle,
) {
    let (lock, changed) = &**scheduler;
    if let Ok(mut control) = lock.lock() {
        control.lifecycle = lifecycle;
        if lifecycle == SchedulerLifecycle::Stopped {
            control.running = false;
            control.handle = None;
        }
        changed.notify_all();
    }
}

fn scheduler_loop(service: &SchomburgService, scheduler: &Arc<(Mutex<SchedulerControl>, Condvar)>) {
    const MAXIMUM_WAIT: Duration = Duration::from_secs(60);
    loop {
        let configuration = match service.get_schedule() {
            Ok(configuration) => configuration,
            Err(_) => {
                set_scheduler_lifecycle(scheduler, SchedulerLifecycle::Failed);
                wait_for_change_or_stop(scheduler, MAXIMUM_WAIT);
                continue;
            }
        };
        if configuration.monitoring == GlobalMonitoringState::Paused {
            set_scheduler_lifecycle(scheduler, SchedulerLifecycle::Paused);
            if wait_for_change_or_stop(scheduler, MAXIMUM_WAIT) {
                break;
            }
            continue;
        }

        let now = SystemTime::now();
        let scheduled_status = match service.store.scheduled_reconciliation_status() {
            Ok(value) => value,
            Err(_) => break,
        };
        let due_run = scheduled_status
            .next_scheduled_run
            .or(configuration.next_run);
        let missed_run = due_run
            .filter(|scheduled| *scheduled <= now)
            .filter(|scheduled| {
                scheduled_status
                    .last_success
                    .is_none_or(|success| success < *scheduled)
            });
        if let Some(missed_run) = missed_run {
            set_scheduler_lifecycle(scheduler, SchedulerLifecycle::Updating);
            let date = record_date_for(missed_run)
                .expect("valid local date")
                .as_str()
                .to_owned();
            let _ = service.scheduled_update(date);
            persist_next_run(service);
            continue;
        }
        let next = match next_eligible_run(&configuration, now) {
            Ok(Some(next)) => next,
            Ok(None) => {
                set_scheduler_lifecycle(scheduler, SchedulerLifecycle::Paused);
                if wait_for_change_or_stop(scheduler, MAXIMUM_WAIT) {
                    break;
                }
                continue;
            }
            Err(_) => {
                set_scheduler_lifecycle(scheduler, SchedulerLifecycle::Failed);
                if wait_for_change_or_stop(scheduler, MAXIMUM_WAIT) {
                    break;
                }
                continue;
            }
        };
        if configuration.next_run != Some(next) {
            let mut updated = configuration;
            updated.next_run = Some(next);
            let _ = service.set_schedule(updated);
        }
        set_scheduler_lifecycle(scheduler, SchedulerLifecycle::Waiting);
        let duration = next
            .duration_since(now)
            .unwrap_or_default()
            .min(MAXIMUM_WAIT);
        if wait_for_change_or_stop(scheduler, duration) {
            break;
        }
        if SystemTime::now() >= next {
            set_scheduler_lifecycle(scheduler, SchedulerLifecycle::Updating);
            let date = record_date_for(next)
                .expect("valid local date")
                .as_str()
                .to_owned();
            let _ = service.scheduled_update(date);
            persist_next_run(service);
        }
    }
    set_scheduler_lifecycle(scheduler, SchedulerLifecycle::Stopped);
}

fn persist_next_run(service: &SchomburgService) {
    if let Ok(mut configuration) = service.get_schedule()
        && let Ok(next) = next_eligible_run(&configuration, SystemTime::now())
    {
        configuration.next_run = next;
        let _ = service.set_schedule(configuration);
        if let Ok(mut scheduled) = service.store.scheduled_reconciliation_status() {
            scheduled.next_scheduled_run = next;
            let _ = service
                .store
                .save_scheduled_reconciliation_status(&scheduled);
        }
    }
}

/// Returns true when shutdown was requested.
fn wait_for_change_or_stop(
    scheduler: &Arc<(Mutex<SchedulerControl>, Condvar)>,
    duration: Duration,
) -> bool {
    let (lock, changed) = &**scheduler;
    let control = match lock.lock() {
        Ok(control) => control,
        Err(_) => return true,
    };
    if control.stop_requested {
        return true;
    }
    match changed.wait_timeout(control, duration) {
        Ok((control, _)) => control.stop_requested,
        Err(_) => true,
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
        time::{Duration, Instant},
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
    fn manual_update_writes_only_manual_operational_status() {
        let directory = TemporaryDirectory::new("manual-status");
        let service = SchomburgService::open(database(&directory)).expect("open");
        service
            .set_record_folder(directory.0.join("records").display().to_string())
            .expect("folder");
        service.update_record(true).expect("manual update");
        let status = service.status().expect("status");
        assert!(status.manual_update.last_success.is_some());
        assert!(status.scheduled_reconciliation.last_success.is_none());
        assert!(
            status
                .scheduled_reconciliation
                .last_reconciled_local_date
                .is_none()
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

    fn wait_for_lifecycle(service: &SchomburgService, expected: SchedulerLifecycle) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if service
                .scheduler_status()
                .expect("scheduler status")
                .lifecycle
                == expected
            {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("scheduler did not reach {expected:?}");
    }

    #[test]
    fn scheduler_pauses_stops_and_rejects_a_second_start() {
        let directory = TemporaryDirectory::new("scheduler-paused");
        let service = SchomburgService::open(database(&directory)).expect("open");
        service.start_scheduler().expect("start");
        wait_for_lifecycle(&service, SchedulerLifecycle::Paused);
        assert!(matches!(
            service.start_scheduler(),
            Err(ServiceError::SchedulerAlreadyRunning)
        ));
        service.stop_scheduler().expect("stop");
        assert_eq!(
            service.scheduler_status().expect("status").lifecycle,
            SchedulerLifecycle::Stopped
        );
        assert!(matches!(
            service.stop_scheduler(),
            Err(ServiceError::SchedulerNotRunning)
        ));
    }

    #[test]
    fn scheduler_catches_up_one_due_run_with_the_existing_update_operation() {
        let directory = TemporaryDirectory::new("scheduler-catch-up");
        let service = SchomburgService::open(database(&directory)).expect("open");
        service
            .set_record_folder(directory.0.join("records").display().to_string())
            .expect("folder");
        service.set_monitoring_enabled(true).expect("enable");
        let mut configuration = service.get_schedule().expect("configuration");
        configuration.next_run = Some(SystemTime::now() - Duration::from_secs(1));
        service
            .store
            .save_reconciliation_configuration(&configuration)
            .expect("due run");

        service.start_scheduler().expect("start");
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if service
                .status()
                .expect("status")
                .configuration
                .last_success
                .is_some()
            {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let status = service.status().expect("status");
        assert!(status.scheduled_reconciliation.last_success.is_some());
        assert_eq!(
            status.scheduled_reconciliation.state,
            schomburg_store::ReconciliationState::Succeeded
        );
        assert!(status.manual_update.last_success.is_none());
        service.stop_scheduler().expect("stop");
    }
}
