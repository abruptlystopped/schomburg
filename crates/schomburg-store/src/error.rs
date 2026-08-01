use crate::{ConnectionId, DiscoveredSourceId};
use schomburg_core::{AnnotationField, AnnotationId, EventId};
use std::fmt;

/// Errors returned by append-only Schomburg storage operations.
#[derive(Debug)]
pub enum StoreError {
    /// The SQLite database could not complete an operation.
    Database {
        message: String,
    },
    /// An event with this stable identifier already exists.
    DuplicateEventId(EventId),
    /// An annotation with this stable identifier already exists.
    DuplicateAnnotationId(AnnotationId),
    /// An annotation's target event does not exist.
    MissingTargetEvent(EventId),
    /// An annotation's superseded predecessor does not exist.
    MissingSupersedesAnnotation(AnnotationId),
    /// A superseded predecessor targets a different event.
    SupersedesDifferentEvent {
        /// The event targeted by the new annotation.
        event_id: EventId,
        /// The event targeted by the predecessor.
        superseded_event_id: EventId,
    },
    /// A superseded predecessor assigns a different organizational field.
    SupersedesDifferentField {
        /// The field assigned by the new annotation.
        field: AnnotationField,
        /// The field assigned by the predecessor.
        superseded_field: AnnotationField,
    },
    /// An annotation cannot name itself as its predecessor.
    SelfSupersession(AnnotationId),
    /// Stored data could not be decoded as a supported representation.
    MalformedStoredData {
        field: &'static str,
        detail: String,
    },
    /// Stored data uses a representation this version does not support.
    UnsupportedStoredData {
        field: &'static str,
        value: String,
    },
    /// A value could not be serialized for storage.
    Serialization {
        detail: String,
    },
    MissingDiscoveredSource(DiscoveredSourceId),
    MissingConnection(ConnectionId),
    ConnectionAlreadyApproved(DiscoveredSourceId),
    ConnectionNotApproved(DiscoveredSourceId),
    InvalidConnectionTransition {
        id: ConnectionId,
        from: crate::ConnectionStatus,
        to: crate::ConnectionStatus,
    },
}

impl StoreError {
    pub(crate) fn database(error: rusqlite::Error) -> Self {
        Self::Database {
            message: error.to_string(),
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database { message } => write!(formatter, "database failure: {message}"),
            Self::DuplicateEventId(id) => write!(formatter, "duplicate event ID: {}", id.as_str()),
            Self::DuplicateAnnotationId(id) => {
                write!(formatter, "duplicate annotation ID: {}", id.as_str())
            }
            Self::MissingTargetEvent(id) => {
                write!(formatter, "missing target event: {}", id.as_str())
            }
            Self::MissingSupersedesAnnotation(id) => {
                write!(formatter, "missing superseded annotation: {}", id.as_str())
            }
            Self::SupersedesDifferentEvent {
                event_id,
                superseded_event_id,
            } => write!(
                formatter,
                "supersession event mismatch: {} cannot supersede an annotation for {}",
                event_id.as_str(),
                superseded_event_id.as_str()
            ),
            Self::SupersedesDifferentField {
                field,
                superseded_field,
            } => write!(
                formatter,
                "supersession field mismatch: {} cannot supersede {}",
                field.as_str(),
                superseded_field.as_str()
            ),
            Self::SelfSupersession(id) => {
                write!(
                    formatter,
                    "annotation cannot supersede itself: {}",
                    id.as_str()
                )
            }
            Self::MalformedStoredData { field, detail } => {
                write!(formatter, "malformed stored {field}: {detail}")
            }
            Self::UnsupportedStoredData { field, value } => {
                write!(formatter, "unsupported stored {field}: {value}")
            }
            Self::Serialization { detail } => write!(formatter, "serialization failure: {detail}"),
            Self::MissingDiscoveredSource(id) => {
                write!(formatter, "missing discovered source: {}", id.as_str())
            }
            Self::MissingConnection(id) => write!(formatter, "missing connection: {}", id.as_str()),
            Self::ConnectionAlreadyApproved(id) => {
                write!(formatter, "source already approved: {}", id.as_str())
            }
            Self::ConnectionNotApproved(id) => write!(
                formatter,
                "source is not awaiting approval: {}",
                id.as_str()
            ),
            Self::InvalidConnectionTransition { id, from, to } => write!(
                formatter,
                "invalid connection transition for {}: {:?} to {:?}",
                id.as_str(),
                from,
                to
            ),
        }
    }
}

impl std::error::Error for StoreError {}
