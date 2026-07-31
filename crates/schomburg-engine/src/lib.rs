//! Engine orchestration for connector-produced Schomburg events.
//!
//! The engine owns connector registration, validates event provenance, and
//! delegates append-only persistence to `schomburg-store`.

use schomburg_connector::{
    Connector, ConnectorDescriptor, ConnectorError, ConnectorRegistry, EventAcceptanceError,
    EventSink, RegistrationError,
};
use schomburg_core::{ConnectorId, Event};
use schomburg_store::Store;
use std::fmt;

/// Coordinates registered connectors with append-only event persistence.
pub struct Engine {
    registry: ConnectorRegistry,
    store: Store,
}

impl Engine {
    /// Creates an engine backed by the supplied append-only store.
    pub fn new(store: Store) -> Self {
        Self {
            registry: ConnectorRegistry::default(),
            store,
        }
    }

    /// Registers immutable connector metadata without taking connector ownership.
    pub fn register(&mut self, descriptor: ConnectorDescriptor) -> Result<(), EngineError> {
        self.registry
            .register(descriptor)
            .map_err(EngineError::Registration)
    }

    /// Runs one registered connector lifecycle and appends accepted events.
    pub fn collect(&self, connector: &mut dyn Connector) -> Result<CollectionReport, EngineError> {
        let descriptor = connector.descriptor().clone();
        let registered = self
            .registry
            .get(descriptor.id())
            .ok_or_else(|| EngineError::UnregisteredConnector(descriptor.id().clone()))?;
        if registered != &descriptor {
            return Err(EngineError::DescriptorMismatch(descriptor.id().clone()));
        }

        let mut sink = EngineEventSink {
            store: &self.store,
            connector_id: descriptor.id().clone(),
            accepted_events: 0,
        };
        connector
            .collect(&mut sink)
            .map_err(EngineError::Connector)?;
        Ok(CollectionReport {
            accepted_events: sink.accepted_events,
        })
    }
}

/// Outcome of one successful connector lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CollectionReport {
    accepted_events: usize,
}

impl CollectionReport {
    /// Returns the number of events accepted and appended during this lifecycle.
    pub fn accepted_events(&self) -> usize {
        self.accepted_events
    }
}

/// Engine-level lifecycle failures.
#[derive(Debug)]
pub enum EngineError {
    /// Connector metadata could not be registered.
    Registration(RegistrationError),
    /// The connector was not registered before collection.
    UnregisteredConnector(ConnectorId),
    /// The running connector no longer matches its registered descriptor.
    DescriptorMismatch(ConnectorId),
    /// The connector lifecycle failed or an emitted event was rejected.
    Connector(ConnectorError),
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registration(error) => write!(formatter, "registration failed: {error}"),
            Self::UnregisteredConnector(id) => {
                write!(formatter, "unregistered connector: {}", id.as_str())
            }
            Self::DescriptorMismatch(id) => {
                write!(formatter, "registered descriptor mismatch: {}", id.as_str())
            }
            Self::Connector(error) => write!(formatter, "connector lifecycle failed: {error}"),
        }
    }
}

impl std::error::Error for EngineError {}

struct EngineEventSink<'a> {
    store: &'a Store,
    connector_id: ConnectorId,
    accepted_events: usize,
}

impl EventSink for EngineEventSink<'_> {
    fn accept(&mut self, event: Event) -> Result<(), EventAcceptanceError> {
        let actual = event.source().connector_id();
        if actual != &self.connector_id {
            return Err(EventAcceptanceError::ConnectorIdentityMismatch {
                expected: self.connector_id.clone(),
                actual: actual.clone(),
            });
        }
        self.store
            .append_event(&event)
            .map_err(|error| EventAcceptanceError::Persistence {
                message: error.to_string(),
            })?;
        self.accepted_events += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schomburg_connector::{ConnectorCapabilities, ConnectorCapability};
    use schomburg_core::{
        CaptureTimestamp, EventId, EventKind, EventPayload, EventTimestamp, SchemaVersion, Source,
        SourceReference,
    };
    use std::{collections::BTreeMap, sync::Arc, time::UNIX_EPOCH};

    struct TestConnector {
        descriptor: ConnectorDescriptor,
        events: Vec<Event>,
    }

    impl TestConnector {
        fn new(id: &str, events: Vec<Event>) -> Self {
            Self {
                descriptor: ConnectorDescriptor::new(
                    ConnectorId::new(id),
                    ConnectorCapabilities::new([ConnectorCapability::new("capture.events")]),
                ),
                events,
            }
        }
    }

    impl Connector for TestConnector {
        fn descriptor(&self) -> &ConnectorDescriptor {
            &self.descriptor
        }

        fn collect(&mut self, sink: &mut dyn EventSink) -> Result<(), ConnectorError> {
            for event in std::mem::take(&mut self.events) {
                sink.accept(event).map_err(ConnectorError::EventRejected)?;
            }
            Ok(())
        }
    }

    fn event(id: &str, connector_id: &str) -> Event {
        Event::new(
            EventId::new(id),
            EventTimestamp::new(UNIX_EPOCH),
            Source::new(
                ConnectorId::new(connector_id),
                SourceReference::new("source-record"),
            ),
            EventKind::new("fixture.event"),
            EventPayload::new(Arc::from([]), None, BTreeMap::new()),
            CaptureTimestamp::new(UNIX_EPOCH),
            SchemaVersion::new("1"),
        )
    }

    #[test]
    fn registered_connector_events_are_validated_and_appended() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", "fixture");
        let mut connector = TestConnector::new("fixture", vec![event.clone()]);
        let mut engine = Engine::new(store);
        engine
            .register(connector.descriptor().clone())
            .expect("register connector");

        let report = engine.collect(&mut connector).expect("collect events");

        assert_eq!(report.accepted_events(), 1);
        assert_eq!(
            engine.store.get_event(event.id()).expect("get event"),
            Some(event)
        );
    }

    #[test]
    fn unregistered_connector_cannot_collect_or_append() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", "fixture");
        let mut connector = TestConnector::new("fixture", vec![event]);
        let engine = Engine::new(store);

        assert!(matches!(
            engine.collect(&mut connector),
            Err(EngineError::UnregisteredConnector(id)) if id.as_str() == "fixture"
        ));
        assert!(engine.store.list_events().expect("list events").is_empty());
    }

    #[test]
    fn provenance_mismatch_is_rejected_without_appending() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", "other-connector");
        let mut connector = TestConnector::new("fixture", vec![event]);
        let mut engine = Engine::new(store);
        engine
            .register(connector.descriptor().clone())
            .expect("register connector");

        assert!(matches!(
            engine.collect(&mut connector),
            Err(EngineError::Connector(ConnectorError::EventRejected(
                EventAcceptanceError::ConnectorIdentityMismatch { expected, actual }
            ))) if expected.as_str() == "fixture" && actual.as_str() == "other-connector"
        ));
        assert!(engine.store.list_events().expect("list events").is_empty());
    }
}
