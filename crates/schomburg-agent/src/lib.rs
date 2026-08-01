//! Portable run-once lifecycle for discovery, consent, and approved collection.

use schomburg_connector::{ConnectorExtension, DiscoveredSourceCandidate};
use schomburg_engine::Engine;
use schomburg_store::{
    ConnectionId, ConnectionStatus, DiscoveredSource, DiscoveredSourceId, Store, StoreError,
};
use std::{collections::BTreeMap, fmt, path::PathBuf, time::SystemTime};

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
#[derive(Debug)]
pub enum AgentError {
    Store(StoreError),
    Extension(schomburg_connector::ExtensionError),
}
impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(e) => write!(f, "agent storage error: {e}"),
            Self::Extension(e) => write!(f, "agent connector error: {e}"),
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
