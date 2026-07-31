//! Immutable domain types for one preserved observed fact.

use std::{collections::BTreeMap, sync::Arc, time::SystemTime};

/// A stable identifier for an event.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EventId(String);

/// An identifier for an optional context associated with an event.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ContextId(String);

/// The stable identifier of the connector that captured a source record.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ConnectorId(String);

/// An uninterpreted label for the kind of observed event.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EventKind(String);

/// An opaque reference to a source record within a connector's domain.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceReference(String);

/// A media type describing an event payload, when the source provides one.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MediaType(String);

/// A source-specific metadata key.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MetadataKey(String);

/// A source-specific metadata value retained without interpretation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MetadataValue(String);

/// The version of the event schema used to create an event.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SchemaVersion(String);

/// The time at which the observed fact occurred according to its source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventTimestamp(SystemTime);

/// The time at which Schomburg captured the observed fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureTimestamp(SystemTime);

/// Provenance for an observed event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    connector_id: ConnectorId,
    reference: SourceReference,
}

/// Source-provided material retained without assigning it meaning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventPayload {
    bytes: Arc<[u8]>,
    media_type: Option<MediaType>,
    metadata: BTreeMap<MetadataKey, MetadataValue>,
}

/// One immutable evidence record for one observed fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    id: EventId,
    occurred_at: EventTimestamp,
    source: Source,
    kind: EventKind,
    context_id: Option<ContextId>,
    payload: EventPayload,
    captured_at: CaptureTimestamp,
    schema_version: SchemaVersion,
}

macro_rules! string_value {
    ($type_name:ident) => {
        impl $type_name {
            /// Creates a value without imposing a format or validation policy.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the retained value.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

string_value!(EventId);
string_value!(ContextId);
string_value!(ConnectorId);
string_value!(EventKind);
string_value!(SourceReference);
string_value!(MediaType);
string_value!(MetadataKey);
string_value!(MetadataValue);
string_value!(SchemaVersion);

impl EventTimestamp {
    /// Creates an event timestamp from a source-provided time.
    pub fn new(value: SystemTime) -> Self {
        Self(value)
    }

    /// Returns the retained source-provided time.
    pub fn as_system_time(&self) -> SystemTime {
        self.0
    }
}

impl CaptureTimestamp {
    /// Creates a capture timestamp.
    pub fn new(value: SystemTime) -> Self {
        Self(value)
    }

    /// Returns the retained capture time.
    pub fn as_system_time(&self) -> SystemTime {
        self.0
    }
}

impl Source {
    /// Creates provenance for a source record.
    pub fn new(connector_id: ConnectorId, reference: SourceReference) -> Self {
        Self {
            connector_id,
            reference,
        }
    }

    /// Returns the connector that captured the record.
    pub fn connector_id(&self) -> &ConnectorId {
        &self.connector_id
    }

    /// Returns the connector-local reference to the source record.
    pub fn reference(&self) -> &SourceReference {
        &self.reference
    }
}

impl EventPayload {
    /// Creates source-provided material without interpreting it.
    pub fn new(
        bytes: Arc<[u8]>,
        media_type: Option<MediaType>,
        metadata: BTreeMap<MetadataKey, MetadataValue>,
    ) -> Self {
        Self {
            bytes,
            media_type,
            metadata,
        }
    }

    /// Returns the retained source bytes.
    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.bytes
    }

    /// Returns the source-provided media type, if any.
    pub fn media_type(&self) -> Option<&MediaType> {
        self.media_type.as_ref()
    }

    /// Returns the retained source-specific metadata.
    pub fn metadata(&self) -> &BTreeMap<MetadataKey, MetadataValue> {
        &self.metadata
    }
}

impl Event {
    /// Creates one immutable record of an observed fact.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: EventId,
        occurred_at: EventTimestamp,
        source: Source,
        kind: EventKind,
        context_id: Option<ContextId>,
        payload: EventPayload,
        captured_at: CaptureTimestamp,
        schema_version: SchemaVersion,
    ) -> Self {
        Self {
            id,
            occurred_at,
            source,
            kind,
            context_id,
            payload,
            captured_at,
            schema_version,
        }
    }

    /// Returns the stable event identifier.
    pub fn id(&self) -> &EventId {
        &self.id
    }

    /// Returns the source-provided occurrence time.
    pub fn occurred_at(&self) -> &EventTimestamp {
        &self.occurred_at
    }

    /// Returns the provenance of this event.
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// Returns the uninterpreted event kind.
    pub fn kind(&self) -> &EventKind {
        &self.kind
    }

    /// Returns the optional context reference.
    pub fn context_id(&self) -> Option<&ContextId> {
        self.context_id.as_ref()
    }

    /// Returns the source-provided payload.
    pub fn payload(&self) -> &EventPayload {
        &self.payload
    }

    /// Returns the time at which Schomburg captured the event.
    pub fn captured_at(&self) -> &CaptureTimestamp {
        &self.captured_at
    }

    /// Returns the schema version used to create the event.
    pub fn schema_version(&self) -> &SchemaVersion {
        &self.schema_version
    }
}
