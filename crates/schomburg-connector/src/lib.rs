//! Connector contracts for producing Schomburg events.
//!
//! This crate deliberately has no storage dependency. Connector implementations
//! produce immutable events through an [`EventSink`]; the engine owns validation
//! and persistence.

use schomburg_core::{ConnectorId, Event, EventId, EventKind};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::PathBuf,
    sync::Arc,
    time::SystemTime,
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

/// A producer and factual presenter of immutable source events.
pub trait Connector: EventPresenter {
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

/// A connector plug-in for agent discovery and approved-connection creation.
///
/// Discovery returns operational candidates only; it must not emit evidence.
pub trait ConnectorExtension {
    /// The stable connector provenance handled by this extension.
    fn connector_id(&self) -> &ConnectorId;

    /// Immutable descriptor used when an approved connector is run.
    fn descriptor(&self) -> ConnectorDescriptor;

    /// Discovers source candidates below explicit host-provided scan roots.
    fn discover(&self, roots: &[PathBuf])
    -> Result<Vec<DiscoveredSourceCandidate>, ExtensionError>;

    /// Creates a collector from connector-owned opaque approved configuration.
    fn open_connection(&self, configuration: &str) -> Result<Box<dyn Connector>, ExtensionError>;
}

/// A connector-owned operational candidate awaiting user consent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredSourceCandidate {
    connector_id: ConnectorId,
    source_identity: String,
    display_name: String,
    local_reference: Option<String>,
    metadata: BTreeMap<String, String>,
    configuration: String,
}

impl DiscoveredSourceCandidate {
    /// Creates an opaque source candidate without evidence collection.
    pub fn new(
        connector_id: ConnectorId,
        source_identity: impl Into<String>,
        display_name: impl Into<String>,
        local_reference: Option<String>,
        metadata: BTreeMap<String, String>,
        configuration: impl Into<String>,
    ) -> Self {
        Self {
            connector_id,
            source_identity: source_identity.into(),
            display_name: display_name.into(),
            local_reference,
            metadata,
            configuration: configuration.into(),
        }
    }
    pub fn connector_id(&self) -> &ConnectorId {
        &self.connector_id
    }
    pub fn source_identity(&self) -> &str {
        &self.source_identity
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn local_reference(&self) -> Option<&str> {
        self.local_reference.as_deref()
    }
    pub fn metadata(&self) -> &BTreeMap<String, String> {
        &self.metadata
    }
    pub fn configuration(&self) -> &str {
        &self.configuration
    }
}

/// A failure from connector-owned discovery or approved-connection opening.
#[derive(Debug)]
pub enum ExtensionError {
    /// A scan root could not be inspected.
    InaccessibleScanRoot { path: PathBuf, message: String },
    /// Source-specific discovery failed.
    DiscoveryFailed { message: String },
    /// Approved configuration could not create a collector.
    ConnectionFailed { message: String },
}

/// Connector-owned factual presentation for one connector's events.
pub trait EventPresenter {
    /// Returns the connector provenance this presenter owns.
    fn connector_id(&self) -> &ConnectorId;

    /// Produces compact factual data for a timeline or list.
    fn present_compact(&self, event: &Event) -> Result<CompactPresentation, PresentationError>;

    /// Produces detailed factual data for inspecting one event.
    fn present_detailed(&self, event: &Event) -> Result<DetailedPresentation, PresentationError>;
}

/// Structured compact factual presentation data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactPresentation {
    source_label: String,
    title: String,
    subtitle: Option<String>,
    timestamp: SystemTime,
    identifiers: Vec<PresentationField>,
    key_fields: Vec<PresentationField>,
}

impl CompactPresentation {
    /// Creates compact factual presentation data with deterministic field order.
    pub fn new(
        source_label: impl Into<String>,
        title: impl Into<String>,
        subtitle: Option<String>,
        timestamp: SystemTime,
        identifiers: Vec<PresentationField>,
        key_fields: Vec<PresentationField>,
    ) -> Self {
        Self {
            source_label: source_label.into(),
            title: title.into(),
            subtitle,
            timestamp,
            identifiers,
            key_fields,
        }
    }

    /// Returns the factual source label.
    pub fn source_label(&self) -> &str {
        &self.source_label
    }

    /// Returns the factual primary title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the optional factual secondary line.
    pub fn subtitle(&self) -> Option<&str> {
        self.subtitle.as_deref()
    }

    /// Returns the event timestamp selected by the connector for display.
    pub fn timestamp(&self) -> SystemTime {
        self.timestamp
    }

    /// Returns short factual identifiers in connector-defined order.
    pub fn identifiers(&self) -> &[PresentationField] {
        &self.identifiers
    }

    /// Returns small factual key fields in connector-defined order.
    pub fn key_fields(&self) -> &[PresentationField] {
        &self.key_fields
    }
}

/// Structured detailed factual presentation data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetailedPresentation {
    source_label: String,
    title: String,
    timestamp: SystemTime,
    fields: Vec<PresentationField>,
    technical_identifiers: Vec<PresentationField>,
    raw_evidence: Option<RawEvidence>,
}

impl DetailedPresentation {
    /// Creates detailed factual presentation data with deterministic field order.
    pub fn new(
        source_label: impl Into<String>,
        title: impl Into<String>,
        timestamp: SystemTime,
        fields: Vec<PresentationField>,
        technical_identifiers: Vec<PresentationField>,
        raw_evidence: Option<RawEvidence>,
    ) -> Self {
        Self {
            source_label: source_label.into(),
            title: title.into(),
            timestamp,
            fields,
            technical_identifiers,
            raw_evidence,
        }
    }

    /// Returns the factual source label.
    pub fn source_label(&self) -> &str {
        &self.source_label
    }

    /// Returns the factual primary title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the event timestamp selected by the connector for display.
    pub fn timestamp(&self) -> SystemTime {
        self.timestamp
    }

    /// Returns ordered factual fields.
    pub fn fields(&self) -> &[PresentationField] {
        &self.fields
    }

    /// Returns ordered technical identifiers.
    pub fn technical_identifiers(&self) -> &[PresentationField] {
        &self.technical_identifiers
    }

    /// Returns exact raw evidence when the connector exposes it.
    pub fn raw_evidence(&self) -> Option<&RawEvidence> {
        self.raw_evidence.as_ref()
    }
}

/// A labeled factual value in a connector-defined order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresentationField {
    label: String,
    value: String,
}

impl PresentationField {
    /// Creates one factual presentation field.
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }

    /// Returns the factual label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the factual value.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Exact evidence that may be rendered only on an explicit raw/debug path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawEvidence {
    label: String,
    media_type: Option<String>,
    bytes: Arc<[u8]>,
}

impl RawEvidence {
    /// Creates a raw evidence section without interpreting its bytes.
    pub fn new(label: impl Into<String>, media_type: Option<String>, bytes: Arc<[u8]>) -> Self {
        Self {
            label: label.into(),
            media_type,
            bytes,
        }
    }

    /// Returns the raw evidence label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the source-provided media type, if any.
    pub fn media_type(&self) -> Option<&str> {
        self.media_type.as_deref()
    }

    /// Returns exact raw evidence bytes.
    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.bytes
    }
}

/// A provenance router for connector-owned presentation, not a presentation engine.
#[derive(Default)]
pub struct PresentationRegistry {
    presenters: BTreeMap<ConnectorId, Box<dyn EventPresenter>>,
}

impl PresentationRegistry {
    /// Registers one connector-owned presenter without overwriting an existing one.
    pub fn register(
        &mut self,
        presenter: Box<dyn EventPresenter>,
    ) -> Result<(), PresentationError> {
        let id = presenter.connector_id().clone();
        if self.presenters.contains_key(&id) {
            return Err(PresentationError::DuplicatePresenter(id));
        }
        self.presenters.insert(id, presenter);
        Ok(())
    }

    /// Routes compact presentation to the presenter that owns the event provenance.
    pub fn present_compact(&self, event: &Event) -> Result<CompactPresentation, PresentationError> {
        self.presenter_for(event)?.present_compact(event)
    }

    /// Routes detailed presentation to the presenter that owns the event provenance.
    pub fn present_detailed(
        &self,
        event: &Event,
    ) -> Result<DetailedPresentation, PresentationError> {
        self.presenter_for(event)?.present_detailed(event)
    }

    fn presenter_for(&self, event: &Event) -> Result<&dyn EventPresenter, PresentationError> {
        self.presenters
            .get(event.source().connector_id())
            .map(Box::as_ref)
            .ok_or_else(|| {
                PresentationError::PresenterNotFound(event.source().connector_id().clone())
            })
    }
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

/// Errors from connector-owned factual presentation.
#[derive(Debug)]
pub enum PresentationError {
    /// No presenter was registered for the event's connector provenance.
    PresenterNotFound(ConnectorId),
    /// A presenter with this provenance is already registered.
    DuplicatePresenter(ConnectorId),
    /// An event belongs to a different connector.
    ConnectorProvenanceMismatch {
        /// The connector provenance owned by the presenter.
        expected: ConnectorId,
        /// The provenance recorded on the event.
        actual: ConnectorId,
    },
    /// The connector does not present this event kind.
    UnsupportedEventKind(EventKind),
    /// The connector payload cannot be decoded as its factual source format.
    MalformedPayload { detail: String },
    /// A factual field needed for presentation is absent.
    MissingFactualField { field: &'static str },
    /// A factual timestamp cannot be converted for presentation.
    InvalidTimestamp { detail: String },
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

impl fmt::Display for ExtensionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InaccessibleScanRoot { path, message } => {
                write!(
                    formatter,
                    "cannot inspect scan root {}: {message}",
                    path.display()
                )
            }
            Self::DiscoveryFailed { message } => {
                write!(formatter, "source discovery failed: {message}")
            }
            Self::ConnectionFailed { message } => write!(formatter, "connection failed: {message}"),
        }
    }
}

impl std::error::Error for ExtensionError {}

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

impl fmt::Display for PresentationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PresenterNotFound(id) => {
                write!(formatter, "no presenter for connector: {}", id.as_str())
            }
            Self::DuplicatePresenter(id) => {
                write!(formatter, "duplicate presenter: {}", id.as_str())
            }
            Self::ConnectorProvenanceMismatch { expected, actual } => write!(
                formatter,
                "event connector {} does not match presenter {}",
                actual.as_str(),
                expected.as_str()
            ),
            Self::UnsupportedEventKind(kind) => {
                write!(formatter, "unsupported event kind: {}", kind.as_str())
            }
            Self::MalformedPayload { detail } => {
                write!(formatter, "malformed connector payload: {detail}")
            }
            Self::MissingFactualField { field } => {
                write!(formatter, "missing factual field: {field}")
            }
            Self::InvalidTimestamp { detail } => {
                write!(formatter, "invalid presentation timestamp: {detail}")
            }
        }
    }
}

impl std::error::Error for PresentationError {}

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
    use schomburg_core::{
        CaptureTimestamp, EventKind, EventPayload, EventTimestamp, SchemaVersion, Source,
        SourceReference,
    };
    use std::{collections::BTreeMap, time::UNIX_EPOCH};

    struct FixturePresenter {
        connector_id: ConnectorId,
    }

    impl EventPresenter for FixturePresenter {
        fn connector_id(&self) -> &ConnectorId {
            &self.connector_id
        }

        fn present_compact(&self, event: &Event) -> Result<CompactPresentation, PresentationError> {
            if event.source().connector_id() != &self.connector_id {
                return Err(PresentationError::ConnectorProvenanceMismatch {
                    expected: self.connector_id.clone(),
                    actual: event.source().connector_id().clone(),
                });
            }
            Ok(CompactPresentation::new(
                "Fixture",
                "event",
                None,
                UNIX_EPOCH,
                Vec::new(),
                Vec::new(),
            ))
        }

        fn present_detailed(
            &self,
            event: &Event,
        ) -> Result<DetailedPresentation, PresentationError> {
            self.present_compact(event)?;
            Ok(DetailedPresentation::new(
                "Fixture",
                "event",
                UNIX_EPOCH,
                Vec::new(),
                Vec::new(),
                None,
            ))
        }
    }

    fn event(connector_id: &str) -> Event {
        Event::new(
            EventId::new("event"),
            EventTimestamp::new(UNIX_EPOCH),
            Source::new(
                ConnectorId::new(connector_id),
                SourceReference::new("source"),
            ),
            EventKind::new("fixture.event"),
            EventPayload::new(Arc::from([]), None, BTreeMap::new()),
            CaptureTimestamp::new(UNIX_EPOCH),
            SchemaVersion::new("1"),
        )
    }

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

    #[test]
    fn presentation_registry_routes_by_provenance_without_source_semantics() {
        let mut registry = PresentationRegistry::default();
        registry
            .register(Box::new(FixturePresenter {
                connector_id: ConnectorId::new("fixture"),
            }))
            .expect("register presenter");

        assert_eq!(
            registry
                .present_compact(&event("fixture"))
                .expect("route presentation")
                .source_label(),
            "Fixture"
        );
        assert!(matches!(
            registry.present_detailed(&event("other")),
            Err(PresentationError::PresenterNotFound(id)) if id.as_str() == "other"
        ));
    }
}
