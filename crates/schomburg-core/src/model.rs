//! Immutable domain types for one preserved observed fact.

use std::{collections::BTreeMap, sync::Arc, time::SystemTime};

/// A stable identifier for an event.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EventId(String);

/// A stable identifier for a future first-class context.
///
/// Context assignments use this type rather than a free-form string so a later
/// context model can enforce referential integrity.
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

/// A stable identifier for an append-only organizational annotation.
///
/// ```compile_fail
/// use schomburg_core::{AnnotationId, EventId};
///
/// fn accepts_event_id(_: EventId) {}
///
/// accepts_event_id(AnnotationId::new("annotation-1"));
/// ```
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AnnotationId(String);

/// An opaque organizational field name.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AnnotationField(String);

/// An organizational value that can retain a typed context reference.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AnnotationValue {
    /// A reference to a future first-class context.
    Context(ContextId),
    /// A value whose domain is intentionally not defined by the core.
    Opaque(String),
}

/// The time at which the observed fact occurred according to its source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventTimestamp(SystemTime);

/// The time at which Schomburg captured the observed fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureTimestamp(SystemTime);

/// The time at which an organizational annotation was assigned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotationTimestamp(SystemTime);

/// The origin of an organizational annotation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentSource {
    /// Assigned by an automated process.
    Automatic,
    /// Assigned by a person.
    User,
    /// Imported from an external record.
    Imported,
    /// Assigned by Schomburg system behavior.
    System,
}

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
///
/// ```compile_fail
/// use schomburg_core::Event;
///
/// let event: Event = unimplemented!();
/// let _ = event.context_id();
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    id: EventId,
    occurred_at: EventTimestamp,
    source: Source,
    kind: EventKind,
    payload: EventPayload,
    captured_at: CaptureTimestamp,
    schema_version: SchemaVersion,
}

/// One immutable, append-only organizational assignment for an event.
///
/// An annotation does not alter its target event. A later annotation may point
/// to an earlier annotation with `supersedes`; selection of a current value is
/// intentionally outside this domain model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Annotation {
    id: AnnotationId,
    event_id: EventId,
    field: AnnotationField,
    value: AnnotationValue,
    source: AssignmentSource,
    assigned_at: AnnotationTimestamp,
    supersedes: Option<AnnotationId>,
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
string_value!(AnnotationId);
string_value!(AnnotationField);

impl AnnotationValue {
    /// Creates a value whose domain is intentionally not defined by the core.
    pub fn opaque(value: impl Into<String>) -> Self {
        Self::Opaque(value.into())
    }
}

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

impl AnnotationTimestamp {
    /// Creates an annotation timestamp.
    pub fn new(value: SystemTime) -> Self {
        Self(value)
    }

    /// Returns the retained assignment time.
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
        payload: EventPayload,
        captured_at: CaptureTimestamp,
        schema_version: SchemaVersion,
    ) -> Self {
        Self {
            id,
            occurred_at,
            source,
            kind,
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

impl Annotation {
    /// Creates one immutable organizational assignment for an event.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: AnnotationId,
        event_id: EventId,
        field: AnnotationField,
        value: AnnotationValue,
        source: AssignmentSource,
        assigned_at: AnnotationTimestamp,
        supersedes: Option<AnnotationId>,
        schema_version: SchemaVersion,
    ) -> Self {
        Self {
            id,
            event_id,
            field,
            value,
            source,
            assigned_at,
            supersedes,
            schema_version,
        }
    }

    /// Returns the stable annotation identifier.
    pub fn id(&self) -> &AnnotationId {
        &self.id
    }

    /// Returns the event this annotation organizes.
    pub fn event_id(&self) -> &EventId {
        &self.event_id
    }

    /// Returns the organizational field this annotation assigns.
    pub fn field(&self) -> &AnnotationField {
        &self.field
    }

    /// Returns the assigned value.
    pub fn value(&self) -> &AnnotationValue {
        &self.value
    }

    /// Returns who or what made the assignment.
    pub fn source(&self) -> AssignmentSource {
        self.source
    }

    /// Returns when the assignment was made.
    pub fn assigned_at(&self) -> &AnnotationTimestamp {
        &self.assigned_at
    }

    /// Returns the prior assignment replaced by this one, if any.
    pub fn supersedes(&self) -> Option<&AnnotationId> {
        self.supersedes.as_ref()
    }

    /// Returns the schema version used to create this annotation.
    pub fn schema_version(&self) -> &SchemaVersion {
        &self.schema_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, sync::Arc, time::UNIX_EPOCH};

    fn event() -> Event {
        Event::new(
            EventId::new("event-1"),
            EventTimestamp::new(UNIX_EPOCH),
            Source::new(ConnectorId::new("test"), SourceReference::new("record-1")),
            EventKind::new("test.record"),
            EventPayload::new(Arc::from([]), None, BTreeMap::new()),
            CaptureTimestamp::new(UNIX_EPOCH),
            SchemaVersion::new("1"),
        )
    }

    fn annotation(
        id: &str,
        event_id: EventId,
        field: &str,
        value: AnnotationValue,
        source: AssignmentSource,
        supersedes: Option<AnnotationId>,
    ) -> Annotation {
        Annotation::new(
            AnnotationId::new(id),
            event_id,
            AnnotationField::new(field),
            value,
            source,
            AnnotationTimestamp::new(UNIX_EPOCH),
            supersedes,
            SchemaVersion::new("1"),
        )
    }

    #[test]
    fn event_contains_only_capture_facts() {
        let event = event();

        assert_eq!(event.id().as_str(), "event-1");
        assert_eq!(event.kind().as_str(), "test.record");
        assert_eq!(event.source().connector_id().as_str(), "test");
    }

    #[test]
    fn annotation_targets_an_event() {
        let assignment = annotation(
            "annotation-1",
            EventId::new("event-1"),
            "context",
            AnnotationValue::Context(ContextId::new("context-1")),
            AssignmentSource::User,
            None,
        );

        assert_eq!(assignment.event_id().as_str(), "event-1");
    }

    #[test]
    fn later_annotation_supersedes_earlier_without_rewriting_it() {
        let earlier = annotation(
            "annotation-1",
            EventId::new("event-1"),
            "context",
            AnnotationValue::Context(ContextId::new("context-1")),
            AssignmentSource::User,
            None,
        );
        let later = annotation(
            "annotation-2",
            EventId::new("event-1"),
            "context",
            AnnotationValue::Context(ContextId::new("context-2")),
            AssignmentSource::User,
            Some(earlier.id().clone()),
        );

        assert_eq!(later.supersedes(), Some(earlier.id()));
        assert_eq!(
            earlier.value(),
            &AnnotationValue::Context(ContextId::new("context-1"))
        );
        assert_eq!(earlier.supersedes(), None);
    }

    #[test]
    fn annotation_preserves_assignment_source() {
        let assignment = annotation(
            "annotation-1",
            EventId::new("event-1"),
            "category",
            AnnotationValue::opaque("research"),
            AssignmentSource::Automatic,
            None,
        );

        assert_eq!(assignment.source(), AssignmentSource::Automatic);
    }

    #[test]
    fn assignment_types_can_coexist_for_an_event() {
        let event_id = EventId::new("event-1");
        let context = annotation(
            "annotation-1",
            event_id.clone(),
            "context",
            AnnotationValue::Context(ContextId::new("context-1")),
            AssignmentSource::User,
            None,
        );
        let tag = annotation(
            "annotation-2",
            event_id,
            "tag",
            AnnotationValue::opaque("important"),
            AssignmentSource::Imported,
            None,
        );

        assert_ne!(context.field(), tag.field());
        assert_eq!(context.event_id(), tag.event_id());
    }
}
