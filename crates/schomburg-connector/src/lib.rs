//! Connector contracts for producing Schomburg events.
//!
//! This crate deliberately has no storage dependency. Connector implementations
//! produce immutable events through an [`EventSink`]; the engine owns validation
//! and persistence.

use schomburg_core::{ConnectorId, Event, EventId};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// An opaque, extensible capability declared by a connector.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ConnectorCapability(String);

impl ConnectorCapability {
    /// Creates a capability identifier without assigning it engine semantics.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the declared capability identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The ordered capabilities declared by a connector.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConnectorCapabilities(BTreeSet<ConnectorCapability>);

impl ConnectorCapabilities {
    /// Creates a deterministic capability set.
    pub fn new(capabilities: impl IntoIterator<Item = ConnectorCapability>) -> Self {
        Self(capabilities.into_iter().collect())
    }

    /// Returns whether this set includes a capability.
    pub fn contains(&self, capability: &ConnectorCapability) -> bool {
        self.0.contains(capability)
    }

    /// Iterates over capabilities in deterministic order.
    pub fn iter(&self) -> impl Iterator<Item = &ConnectorCapability> {
        self.0.iter()
    }
}

/// Immutable metadata used to register and identify a connector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectorDescriptor {
    id: ConnectorId,
    capabilities: ConnectorCapabilities,
}

impl ConnectorDescriptor {
    /// Creates connector registration metadata.
    pub fn new(id: ConnectorId, capabilities: ConnectorCapabilities) -> Self {
        Self { id, capabilities }
    }

    /// Returns the connector's stable identifier.
    pub fn id(&self) -> &ConnectorId {
        &self.id
    }

    /// Returns the capabilities declared at registration.
    pub fn capabilities(&self) -> &ConnectorCapabilities {
        &self.capabilities
    }
}

/// Registry of connector descriptors. It does not own connector instances.
#[derive(Debug, Default)]
pub struct ConnectorRegistry {
    descriptors: BTreeMap<ConnectorId, ConnectorDescriptor>,
}

impl ConnectorRegistry {
    /// Registers a descriptor once. Existing registrations are not overwritten.
    pub fn register(&mut self, descriptor: ConnectorDescriptor) -> Result<(), RegistrationError> {
        let id = descriptor.id().clone();
        if self.descriptors.contains_key(&id) {
            return Err(RegistrationError::DuplicateConnectorId(id));
        }
        self.descriptors.insert(id, descriptor);
        Ok(())
    }

    /// Returns a descriptor by connector identifier.
    pub fn get(&self, id: &ConnectorId) -> Option<&ConnectorDescriptor> {
        self.descriptors.get(id)
    }
}

/// A producer of immutable source events.
pub trait Connector {
    /// Returns immutable registration metadata for this connector.
    fn descriptor(&self) -> &ConnectorDescriptor;

    /// Produces events through the supplied engine-owned sink.
    fn collect(&mut self, sink: &mut dyn EventSink) -> Result<(), ConnectorError>;
}

/// An engine-owned destination for events emitted by one connector run.
pub trait EventSink {
    /// Accepts an immutable event for validation and append-only persistence.
    fn accept(&mut self, event: Event) -> Result<(), EventAcceptanceError>;
}

/// A connector-reported failure.
#[derive(Debug)]
pub enum ConnectorError {
    /// The connector could not collect source evidence.
    CollectionFailed { message: String },
    /// The engine rejected an event emitted by the connector.
    EventRejected(EventAcceptanceError),
}

impl ConnectorError {
    /// Creates a collection failure without exposing source-specific error types.
    pub fn collection_failed(message: impl Into<String>) -> Self {
        Self::CollectionFailed {
            message: message.into(),
        }
    }
}

/// An error returned when the engine cannot accept a connector-produced event.
#[derive(Debug)]
pub enum EventAcceptanceError {
    /// The event provenance does not match the connector run that emitted it.
    ConnectorIdentityMismatch {
        /// The registered and running connector identifier.
        expected: ConnectorId,
        /// The connector identifier recorded in the event provenance.
        actual: ConnectorId,
    },
    /// An event with this stable identifier was already preserved.
    DuplicateEventId(EventId),
    /// Append-only persistence rejected the event.
    Persistence { message: String },
}

/// A descriptor registration failure.
#[derive(Debug)]
pub enum RegistrationError {
    /// A descriptor with this identifier is already registered.
    DuplicateConnectorId(ConnectorId),
}

impl fmt::Display for ConnectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CollectionFailed { message } => write!(formatter, "collection failed: {message}"),
            Self::EventRejected(error) => write!(formatter, "event rejected: {error}"),
        }
    }
}

impl std::error::Error for ConnectorError {}

impl fmt::Display for EventAcceptanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectorIdentityMismatch { expected, actual } => write!(
                formatter,
                "event connector ID {} does not match running connector {}",
                actual.as_str(),
                expected.as_str()
            ),
            Self::DuplicateEventId(id) => write!(formatter, "duplicate event ID: {}", id.as_str()),
            Self::Persistence { message } => write!(formatter, "persistence failed: {message}"),
        }
    }
}

impl std::error::Error for EventAcceptanceError {}

impl fmt::Display for RegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateConnectorId(id) => {
                write!(formatter, "duplicate connector ID: {}", id.as_str())
            }
        }
    }
}

impl std::error::Error for RegistrationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_preserves_capabilities_and_rejects_duplicates() {
        let id = ConnectorId::new("fixture");
        let capability = ConnectorCapability::new("capture.events");
        let descriptor =
            ConnectorDescriptor::new(id.clone(), ConnectorCapabilities::new([capability.clone()]));
        let mut registry = ConnectorRegistry::default();
        registry
            .register(descriptor.clone())
            .expect("register descriptor");

        assert!(
            registry
                .get(&id)
                .expect("registered descriptor")
                .capabilities()
                .contains(&capability)
        );
        assert!(matches!(
            registry.register(descriptor),
            Err(RegistrationError::DuplicateConnectorId(duplicate)) if duplicate == id
        ));
    }
}
