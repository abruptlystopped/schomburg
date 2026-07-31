//! Domain model for Schomburg evidence records.
//!
//! This crate contains data types only. It does not collect, persist,
//! interpret, or present evidence.

pub mod model;

pub use model::{
    Annotation, AnnotationField, AnnotationId, AnnotationTimestamp, AnnotationValue,
    AssignmentSource, CaptureTimestamp, ConnectorId, ContextId, Event, EventId, EventKind,
    EventPayload, EventTimestamp, MediaType, MetadataKey, MetadataValue, SchemaVersion, Source,
    SourceReference,
};
