//! Append-only local SQLite persistence for Schomburg domain records.
//!
//! SQLite implementation details are private to this crate. The public API
//! intentionally exposes append, get, and list operations only.

mod error;
mod migrations;
mod operations;
mod serialization;

pub use error::StoreError;
pub use operations::*;

use migrations::apply as apply_migrations;
use rusqlite::{Connection, OptionalExtension, Row, params};
use schomburg_core::{
    Annotation, AnnotationField, AnnotationId, AnnotationTimestamp, AnnotationValue,
    AssignmentSource, CaptureTimestamp, ConnectorId, ContextId, Event, EventId, EventKind,
    EventPayload, EventTimestamp, MediaType, SchemaVersion, Source, SourceReference,
};
use serialization::{decode_metadata, decode_timestamp, encode_metadata, encode_timestamp};
use std::{path::Path, sync::Arc, time::SystemTime};

/// An embedded, append-only store for Schomburg events and annotations.
///
/// ```compile_fail
/// use schomburg_store::Store;
///
/// let store: Store = unimplemented!();
/// store.update_event();
/// ```
///
/// ```compile_fail
/// use schomburg_store::Store;
///
/// let store: Store = unimplemented!();
/// store.delete_annotation();
/// ```
pub struct Store {
    connection: Connection,
}

impl Store {
    /// Opens a local database and applies all pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let mut connection = Connection::open(path).map_err(StoreError::database)?;
        apply_migrations(&mut connection)?;
        Ok(Self { connection })
    }

    /// Opens an in-memory database and applies all pending migrations.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let mut connection = Connection::open_in_memory().map_err(StoreError::database)?;
        apply_migrations(&mut connection)?;
        Ok(Self { connection })
    }

    /// Appends an event. Existing events are never overwritten.
    pub fn append_event(&self, event: &Event) -> Result<(), StoreError> {
        let changed = self
            .connection
            .execute(
                "
                INSERT INTO events (
                    id, occurred_at, captured_at, connector_id, source_reference,
                    event_kind, payload_bytes, payload_media_type,
                    payload_metadata_json, schema_version
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(id) DO NOTHING
                ",
                params![
                    event.id().as_str(),
                    encode_timestamp(event.occurred_at().as_system_time()).as_slice(),
                    encode_timestamp(event.captured_at().as_system_time()).as_slice(),
                    event.source().connector_id().as_str(),
                    event.source().reference().as_str(),
                    event.kind().as_str(),
                    event.payload().bytes().as_ref(),
                    event.payload().media_type().map(MediaType::as_str),
                    encode_metadata(event.payload().metadata())?,
                    event.schema_version().as_str(),
                ],
            )
            .map_err(StoreError::database)?;

        if changed == 0 {
            return Err(StoreError::DuplicateEventId(event.id().clone()));
        }
        Ok(())
    }

    /// Retrieves an event by its stable identifier.
    pub fn get_event(&self, id: &EventId) -> Result<Option<Event>, StoreError> {
        let row = self
            .connection
            .query_row(
                "
                SELECT id, occurred_at, captured_at, connector_id, source_reference,
                    event_kind, payload_bytes, payload_media_type,
                    payload_metadata_json, schema_version
                FROM events WHERE id = ?1
                ",
                [id.as_str()],
                StoredEvent::from_row,
            )
            .optional()
            .map_err(StoreError::database)?;
        row.map(StoredEvent::into_event).transpose()
    }

    /// Lists events by occurrence time, then stable identifier.
    pub fn list_events(&self) -> Result<Vec<Event>, StoreError> {
        let mut statement = self
            .connection
            .prepare(
                "
                SELECT id, occurred_at, captured_at, connector_id, source_reference,
                    event_kind, payload_bytes, payload_media_type,
                    payload_metadata_json, schema_version
                FROM events ORDER BY occurred_at ASC, id ASC
                ",
            )
            .map_err(StoreError::database)?;
        let mut rows = statement.query([]).map_err(StoreError::database)?;
        let mut events = Vec::new();
        while let Some(row) = rows.next().map_err(StoreError::database)? {
            events.push(
                StoredEvent::from_row(row)
                    .map_err(StoreError::database)?
                    .into_event()?,
            );
        }
        Ok(events)
    }

    /// Appends an annotation. Existing annotations are never overwritten.
    pub fn append_annotation(&self, annotation: &Annotation) -> Result<(), StoreError> {
        if !self.event_exists(annotation.event_id())? {
            return Err(StoreError::MissingTargetEvent(
                annotation.event_id().clone(),
            ));
        }
        if let Some(supersedes) = annotation.supersedes() {
            if supersedes == annotation.id() {
                return Err(StoreError::SelfSupersession(annotation.id().clone()));
            }
            let predecessor = self
                .get_annotation(supersedes)?
                .ok_or_else(|| StoreError::MissingSupersedesAnnotation(supersedes.clone()))?;
            if predecessor.event_id() != annotation.event_id() {
                return Err(StoreError::SupersedesDifferentEvent {
                    event_id: annotation.event_id().clone(),
                    superseded_event_id: predecessor.event_id().clone(),
                });
            }
            if predecessor.field() != annotation.field() {
                return Err(StoreError::SupersedesDifferentField {
                    field: annotation.field().clone(),
                    superseded_field: predecessor.field().clone(),
                });
            }
        }

        let (value_kind, value_text) = match annotation.value() {
            AnnotationValue::Context(id) => ("context", id.as_str()),
            AnnotationValue::Opaque(value) => ("opaque", value.as_str()),
        };
        let changed = self
            .connection
            .execute(
                "
                INSERT INTO annotations (
                    id, event_id, field, value_kind, value_text, assignment_source,
                    assigned_at, supersedes_annotation_id, schema_version
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(id) DO NOTHING
                ",
                params![
                    annotation.id().as_str(),
                    annotation.event_id().as_str(),
                    annotation.field().as_str(),
                    value_kind,
                    value_text,
                    assignment_source_to_str(annotation.source()),
                    encode_timestamp(annotation.assigned_at().as_system_time()).as_slice(),
                    annotation.supersedes().map(AnnotationId::as_str),
                    annotation.schema_version().as_str(),
                ],
            )
            .map_err(StoreError::database)?;

        if changed == 0 {
            return Err(StoreError::DuplicateAnnotationId(annotation.id().clone()));
        }
        Ok(())
    }

    /// Retrieves an annotation by its stable identifier.
    pub fn get_annotation(&self, id: &AnnotationId) -> Result<Option<Annotation>, StoreError> {
        let row = self
            .connection
            .query_row(
                "
                SELECT id, event_id, field, value_kind, value_text, assignment_source,
                    assigned_at, supersedes_annotation_id, schema_version
                FROM annotations WHERE id = ?1
                ",
                [id.as_str()],
                StoredAnnotation::from_row,
            )
            .optional()
            .map_err(StoreError::database)?;
        row.map(StoredAnnotation::into_annotation).transpose()
    }

    /// Lists an event's annotations by assignment time, then stable identifier.
    pub fn list_annotations_for_event(
        &self,
        event_id: &EventId,
    ) -> Result<Vec<Annotation>, StoreError> {
        let mut statement = self
            .connection
            .prepare(
                "
                SELECT id, event_id, field, value_kind, value_text, assignment_source,
                    assigned_at, supersedes_annotation_id, schema_version
                FROM annotations WHERE event_id = ?1
                ORDER BY assigned_at ASC, id ASC
                ",
            )
            .map_err(StoreError::database)?;
        let mut rows = statement
            .query([event_id.as_str()])
            .map_err(StoreError::database)?;
        let mut annotations = Vec::new();
        while let Some(row) = rows.next().map_err(StoreError::database)? {
            annotations.push(
                StoredAnnotation::from_row(row)
                    .map_err(StoreError::database)?
                    .into_annotation()?,
            );
        }
        Ok(annotations)
    }

    fn event_exists(&self, id: &EventId) -> Result<bool, StoreError> {
        self.connection
            .query_row("SELECT 1 FROM events WHERE id = ?1", [id.as_str()], |_| {
                Ok(())
            })
            .optional()
            .map(|result| result.is_some())
            .map_err(StoreError::database)
    }

    /// Creates or refreshes a connector-owned discovery candidate without collecting evidence.
    pub fn upsert_discovered_source(
        &self,
        source: &DiscoveredSource,
    ) -> Result<DiscoveredSource, StoreError> {
        let metadata =
            serde_json::to_string(&source.metadata).map_err(|error| StoreError::Serialization {
                detail: error.to_string(),
            })?;
        self.connection.execute("INSERT INTO discovered_sources (id,connector_id,source_identity,display_name,local_reference,first_discovered,last_seen,status,metadata_json,configuration) VALUES (?1,?2,?3,?4,?5,?6,?7,'awaiting',?8,?9) ON CONFLICT(connector_id,source_identity) DO UPDATE SET display_name=excluded.display_name,local_reference=excluded.local_reference,last_seen=excluded.last_seen,metadata_json=excluded.metadata_json,configuration=excluded.configuration", params![source.id.as_str(), source.connector_id.as_str(), source.source_identity, source.display_name, source.local_reference, encode_timestamp(source.first_discovered).as_slice(), encode_timestamp(source.last_seen).as_slice(), metadata, source.configuration]).map_err(StoreError::database)?;
        self.get_discovered_by_identity(&source.connector_id, &source.source_identity)?
            .ok_or(StoreError::MalformedStoredData {
                field: "discovered source",
                detail: "upsert returned no row".to_owned(),
            })
    }
    pub fn list_discovered_sources(&self) -> Result<Vec<DiscoveredSource>, StoreError> {
        self.read_sources("SELECT id,connector_id,source_identity,display_name,local_reference,first_discovered,last_seen,status,metadata_json,configuration FROM discovered_sources ORDER BY first_discovered,id", [])
    }
    pub fn get_discovered_source(
        &self,
        id: &DiscoveredSourceId,
    ) -> Result<Option<DiscoveredSource>, StoreError> {
        self.connection.query_row("SELECT id,connector_id,source_identity,display_name,local_reference,first_discovered,last_seen,status,metadata_json,configuration FROM discovered_sources WHERE id=?1", [id.as_str()], stored_source).optional().map_err(StoreError::database)?.map(decode_source).transpose()
    }
    fn get_discovered_by_identity(
        &self,
        connector: &ConnectorId,
        identity: &str,
    ) -> Result<Option<DiscoveredSource>, StoreError> {
        self.connection.query_row("SELECT id,connector_id,source_identity,display_name,local_reference,first_discovered,last_seen,status,metadata_json,configuration FROM discovered_sources WHERE connector_id=?1 AND source_identity=?2", params![connector.as_str(),identity], stored_source).optional().map_err(StoreError::database)?.map(decode_source).transpose()
    }
    fn read_sources<P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
    ) -> Result<Vec<DiscoveredSource>, StoreError> {
        let mut statement = self.connection.prepare(sql).map_err(StoreError::database)?;
        let rows = statement
            .query_map(params, stored_source)
            .map_err(StoreError::database)?;
        rows.map(|row| row.map_err(StoreError::database).and_then(decode_source))
            .collect()
    }
    pub fn approve_source(
        &self,
        source_id: &DiscoveredSourceId,
        connection_id: &ConnectionId,
        now: std::time::SystemTime,
    ) -> Result<ConnectionRecord, StoreError> {
        let source = self
            .get_discovered_source(source_id)?
            .ok_or(StoreError::MissingDiscoveredSource(source_id.clone()))?;
        if source.status == DiscoveryStatus::Connected {
            return Err(StoreError::ConnectionAlreadyApproved(source_id.clone()));
        }
        if source.status == DiscoveryStatus::Declined {
            return Err(StoreError::ConnectionNotApproved(source_id.clone()));
        }
        self.connection
            .execute(
                "UPDATE discovered_sources SET status='connected' WHERE id=?1",
                [source_id.as_str()],
            )
            .map_err(StoreError::database)?;
        self.connection.execute("INSERT INTO connections (id,source_id,connector_id,status,policy,approved_at,configuration) VALUES (?1,?2,?3,'enabled','always',?4,?5)",params![connection_id.as_str(),source_id.as_str(),source.connector_id.as_str(),encode_timestamp(now).as_slice(),source.configuration]).map_err(StoreError::database)?;
        self.get_connection(connection_id)?
            .ok_or(StoreError::MalformedStoredData {
                field: "connection",
                detail: "approval returned no row".to_owned(),
            })
    }
    pub fn decline_source(&self, id: &DiscoveredSourceId) -> Result<(), StoreError> {
        if self.get_discovered_source(id)?.is_none() {
            return Err(StoreError::MissingDiscoveredSource(id.clone()));
        }
        self.connection
            .execute(
                "UPDATE discovered_sources SET status='declined' WHERE id=?1",
                [id.as_str()],
            )
            .map_err(StoreError::database)?;
        Ok(())
    }
    pub fn list_connections(&self) -> Result<Vec<ConnectionRecord>, StoreError> {
        let mut s=self.connection.prepare("SELECT id,source_id,connector_id,status,policy,approved_at,revoked_at,configuration,last_attempt,last_success,last_error FROM connections ORDER BY approved_at,id").map_err(StoreError::database)?;
        s.query_map([], stored_connection)
            .map_err(StoreError::database)?
            .map(|r| r.map_err(StoreError::database).and_then(decode_connection))
            .collect()
    }
    pub fn get_connection(
        &self,
        id: &ConnectionId,
    ) -> Result<Option<ConnectionRecord>, StoreError> {
        self.connection.query_row("SELECT id,source_id,connector_id,status,policy,approved_at,revoked_at,configuration,last_attempt,last_success,last_error FROM connections WHERE id=?1",[id.as_str()],stored_connection).optional().map_err(StoreError::database)?.map(decode_connection).transpose()
    }
    pub fn set_connection_status(
        &self,
        id: &ConnectionId,
        status: ConnectionStatus,
        now: SystemTime,
    ) -> Result<(), StoreError> {
        let current = self
            .get_connection(id)?
            .ok_or(StoreError::MissingConnection(id.clone()))?;
        let valid = matches!(
            (current.status, status),
            (
                ConnectionStatus::Enabled,
                ConnectionStatus::Paused | ConnectionStatus::Disconnected
            ) | (
                ConnectionStatus::Paused,
                ConnectionStatus::Enabled | ConnectionStatus::Disconnected
            )
        );
        if !valid {
            return Err(StoreError::InvalidConnectionTransition {
                id: id.clone(),
                from: current.status,
                to: status,
            });
        }
        self.connection.execute("UPDATE connections SET status=?2,policy=?3,revoked_at=CASE WHEN ?2='disconnected' THEN ?4 ELSE revoked_at END WHERE id=?1",params![id.as_str(),connection_status(status),policy_for(status),encode_timestamp(now).as_slice()]).map_err(StoreError::database)?;
        Ok(())
    }
    pub fn update_connection_result(
        &self,
        id: &ConnectionId,
        now: SystemTime,
        success: bool,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        let changed=self.connection.execute("UPDATE connections SET last_attempt=?2,last_success=CASE WHEN ?3 THEN ?2 ELSE last_success END,last_error=?4 WHERE id=?1",params![id.as_str(),encode_timestamp(now).as_slice(),success,error]).map_err(StoreError::database)?;
        if changed == 0 {
            Err(StoreError::MissingConnection(id.clone()))
        } else {
            Ok(())
        }
    }

    /// Reads the singleton reconciliation operational configuration.
    pub fn reconciliation_configuration(&self) -> Result<ReconciliationConfiguration, StoreError> {
        self.connection.query_row("SELECT monitoring,record_folder,schedule_kind,schedule_days,local_hour,local_minute,last_attempt,last_success,next_run,last_error,imported,duplicates,rejected,failed,state FROM reconciliation_configuration WHERE id=1",[],|r| Ok((r.get::<_,String>(0)?,r.get(1)?,r.get::<_,String>(2)?,r.get::<_,u8>(3)?,r.get::<_,u8>(4)?,r.get::<_,u8>(5)?,r.get::<_,Option<Vec<u8>>>(6)?,r.get::<_,Option<Vec<u8>>>(7)?,r.get::<_,Option<Vec<u8>>>(8)?,r.get(9)?,r.get::<_,i64>(10)? as u64,r.get::<_,i64>(11)? as u64,r.get::<_,i64>(12)? as u64,r.get::<_,i64>(13)? as u64,r.get::<_,String>(14)?))).map_err(StoreError::database).and_then(|v| {let(monitoring,record_folder,kind,days,hour,minute,a,s,n,error,i,d,r,f,state)=v;Ok(ReconciliationConfiguration{monitoring:if monitoring=="enabled"{GlobalMonitoringState::Enabled}else{GlobalMonitoringState::Paused},record_folder,schedule:match kind.as_str(){"daily"=>ReconciliationSchedule::Daily,"weekdays"=>ReconciliationSchedule::Weekdays,"selected"=>ReconciliationSchedule::Selected(days),_=>return Err(StoreError::UnsupportedStoredData{field:"schedule",value:kind})},time:LocalReconciliationTime{hour,minute},last_attempt:a.map(|x|decode_timestamp(&x,"last attempt")).transpose()?,last_success:s.map(|x|decode_timestamp(&x,"last success")).transpose()?,next_run:n.map(|x|decode_timestamp(&x,"next run")).transpose()?,last_error:error,counts:ReconciliationCounts{imported:i,duplicates:d,rejected:r,failed:f},state:match state.as_str(){"idle"=>ReconciliationState::Idle,"running"=>ReconciliationState::Running,"succeeded"=>ReconciliationState::Succeeded,"failed"=>ReconciliationState::Failed,_=>ReconciliationState::Paused}})})
    }
    pub fn save_reconciliation_configuration(
        &self,
        value: &ReconciliationConfiguration,
    ) -> Result<(), StoreError> {
        let (kind, days) = match value.schedule {
            ReconciliationSchedule::Daily => ("daily", 0),
            ReconciliationSchedule::Weekdays => ("weekdays", 0),
            ReconciliationSchedule::Selected(d) => ("selected", d),
        };
        self.connection.execute("UPDATE reconciliation_configuration SET monitoring=?1,record_folder=?2,schedule_kind=?3,schedule_days=?4,local_hour=?5,local_minute=?6,last_attempt=?7,last_success=?8,next_run=?9,last_error=?10,imported=?11,duplicates=?12,rejected=?13,failed=?14,state=?15 WHERE id=1",params![if value.monitoring==GlobalMonitoringState::Enabled{"enabled"}else{"paused"},value.record_folder,kind,days,value.time.hour,value.time.minute,value.last_attempt.map(encode_timestamp),value.last_success.map(encode_timestamp),value.next_run.map(encode_timestamp),value.last_error,value.counts.imported as i64,value.counts.duplicates as i64,value.counts.rejected as i64,value.counts.failed as i64,match value.state{ReconciliationState::Idle=>"idle",ReconciliationState::Running=>"running",ReconciliationState::Succeeded=>"succeeded",ReconciliationState::Failed=>"failed",ReconciliationState::Paused=>"paused"}]).map_err(StoreError::database)?;
        Ok(())
    }
    pub fn manual_update_status(&self) -> Result<ManualUpdateStatus, StoreError> {
        self.read_manual_status("manual_update_status")
    }
    pub fn save_manual_update_status(&self, value: &ManualUpdateStatus) -> Result<(), StoreError> {
        self.write_manual_status("manual_update_status", value)
    }
    pub fn scheduled_reconciliation_status(
        &self,
    ) -> Result<ScheduledReconciliationStatus, StoreError> {
        self.connection.query_row("SELECT last_attempt,last_success,last_error,imported,duplicates,rejected,failed,state,last_reconciled_local_date,next_scheduled_run FROM scheduled_reconciliation_status WHERE id=1",[],|r|Ok((r.get::<_,Option<Vec<u8>>>(0)?,r.get::<_,Option<Vec<u8>>>(1)?,r.get(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?,r.get::<_,String>(7)?,r.get(8)?,r.get::<_,Option<Vec<u8>>>(9)?))).map_err(StoreError::database).and_then(|(a,s,e,i,d,r,f,state,date,next)|Ok(ScheduledReconciliationStatus{last_attempt:a.map(|v|decode_timestamp(&v,"scheduled attempt")).transpose()?,last_success:s.map(|v|decode_timestamp(&v,"scheduled success")).transpose()?,last_error:e,counts:ReconciliationCounts{imported:i as u64,duplicates:d as u64,rejected:r as u64,failed:f as u64},state:decode_state(&state),last_reconciled_local_date:date,next_scheduled_run:next.map(|v|decode_timestamp(&v,"next scheduled run")).transpose()?}))
    }
    pub fn save_scheduled_reconciliation_status(
        &self,
        value: &ScheduledReconciliationStatus,
    ) -> Result<(), StoreError> {
        self.connection.execute("UPDATE scheduled_reconciliation_status SET last_attempt=?1,last_success=?2,last_error=?3,imported=?4,duplicates=?5,rejected=?6,failed=?7,state=?8,last_reconciled_local_date=?9,next_scheduled_run=?10 WHERE id=1",params![value.last_attempt.map(encode_timestamp),value.last_success.map(encode_timestamp),value.last_error,value.counts.imported as i64,value.counts.duplicates as i64,value.counts.rejected as i64,value.counts.failed as i64,encode_state(value.state),value.last_reconciled_local_date,value.next_scheduled_run.map(encode_timestamp)]).map_err(StoreError::database)?;
        Ok(())
    }
    fn read_manual_status(&self, table: &str) -> Result<ManualUpdateStatus, StoreError> {
        let sql = format!(
            "SELECT last_attempt,last_success,last_error,imported,duplicates,rejected,failed,state FROM {table} WHERE id=1"
        );
        self.connection
            .query_row(&sql, [], |r| {
                Ok((
                    r.get::<_, Option<Vec<u8>>>(0)?,
                    r.get::<_, Option<Vec<u8>>>(1)?,
                    r.get(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, String>(7)?,
                ))
            })
            .map_err(StoreError::database)
            .and_then(|(a, s, e, i, d, r, f, state)| {
                Ok(ManualUpdateStatus {
                    last_attempt: a
                        .map(|v| decode_timestamp(&v, "manual attempt"))
                        .transpose()?,
                    last_success: s
                        .map(|v| decode_timestamp(&v, "manual success"))
                        .transpose()?,
                    last_error: e,
                    counts: ReconciliationCounts {
                        imported: i as u64,
                        duplicates: d as u64,
                        rejected: r as u64,
                        failed: f as u64,
                    },
                    state: decode_state(&state),
                })
            })
    }
    fn write_manual_status(
        &self,
        table: &str,
        value: &ManualUpdateStatus,
    ) -> Result<(), StoreError> {
        let sql = format!(
            "UPDATE {table} SET last_attempt=?1,last_success=?2,last_error=?3,imported=?4,duplicates=?5,rejected=?6,failed=?7,state=?8 WHERE id=1"
        );
        self.connection
            .execute(
                &sql,
                params![
                    value.last_attempt.map(encode_timestamp),
                    value.last_success.map(encode_timestamp),
                    value.last_error,
                    value.counts.imported as i64,
                    value.counts.duplicates as i64,
                    value.counts.rejected as i64,
                    value.counts.failed as i64,
                    encode_state(value.state)
                ],
            )
            .map_err(StoreError::database)?;
        Ok(())
    }
}

fn encode_state(value: ReconciliationState) -> &'static str {
    match value {
        ReconciliationState::Idle => "idle",
        ReconciliationState::Running => "running",
        ReconciliationState::Succeeded => "succeeded",
        ReconciliationState::Failed => "failed",
        ReconciliationState::Paused => "paused",
    }
}
fn decode_state(value: &str) -> ReconciliationState {
    match value {
        "idle" => ReconciliationState::Idle,
        "running" => ReconciliationState::Running,
        "succeeded" => ReconciliationState::Succeeded,
        "failed" => ReconciliationState::Failed,
        _ => ReconciliationState::Paused,
    }
}

struct StoredEvent {
    id: String,
    occurred_at: Vec<u8>,
    captured_at: Vec<u8>,
    connector_id: String,
    source_reference: String,
    event_kind: String,
    payload_bytes: Vec<u8>,
    payload_media_type: Option<String>,
    payload_metadata_json: String,
    schema_version: String,
}

type StoredSource = (
    String,
    String,
    String,
    String,
    Option<String>,
    Vec<u8>,
    Vec<u8>,
    String,
    String,
    String,
);
fn stored_source(row: &Row<'_>) -> rusqlite::Result<StoredSource> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}
fn decode_source(value: StoredSource) -> Result<DiscoveredSource, StoreError> {
    let (
        id,
        connector_id,
        source_identity,
        display_name,
        local_reference,
        first,
        last,
        status,
        metadata,
        configuration,
    ) = value;
    let metadata =
        serde_json::from_str(&metadata).map_err(|e| StoreError::MalformedStoredData {
            field: "discovered source metadata",
            detail: e.to_string(),
        })?;
    Ok(DiscoveredSource {
        id: DiscoveredSourceId::new(id),
        connector_id: ConnectorId::new(connector_id),
        source_identity,
        display_name,
        local_reference,
        first_discovered: decode_timestamp(&first, "source first discovered")?,
        last_seen: decode_timestamp(&last, "source last seen")?,
        status: match status.as_str() {
            "awaiting" => DiscoveryStatus::AwaitingDecision,
            "declined" => DiscoveryStatus::Declined,
            "connected" => DiscoveryStatus::Connected,
            _ => {
                return Err(StoreError::UnsupportedStoredData {
                    field: "discovery status",
                    value: status,
                });
            }
        },
        metadata,
        configuration,
    })
}
type StoredConnection = (
    String,
    String,
    String,
    String,
    String,
    Vec<u8>,
    Option<Vec<u8>>,
    String,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<String>,
);
fn stored_connection(row: &Row<'_>) -> rusqlite::Result<StoredConnection> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
    ))
}
fn decode_connection(v: StoredConnection) -> Result<ConnectionRecord, StoreError> {
    let (
        id,
        source_id,
        connector_id,
        status,
        policy,
        approved,
        revoked,
        configuration,
        attempt,
        success,
        last_error,
    ) = v;
    Ok(ConnectionRecord {
        id: ConnectionId::new(id),
        source_id: DiscoveredSourceId::new(source_id),
        connector_id: ConnectorId::new(connector_id),
        status: match status.as_str() {
            "enabled" => ConnectionStatus::Enabled,
            "paused" => ConnectionStatus::Paused,
            "disconnected" => ConnectionStatus::Disconnected,
            _ => {
                return Err(StoreError::UnsupportedStoredData {
                    field: "connection status",
                    value: status,
                });
            }
        },
        policy: match policy.as_str() {
            "always" => MonitoringPolicy::Always,
            "paused" => MonitoringPolicy::Paused,
            _ => {
                return Err(StoreError::UnsupportedStoredData {
                    field: "monitoring policy",
                    value: policy,
                });
            }
        },
        approved_at: decode_timestamp(&approved, "connection approved")?,
        revoked_at: revoked
            .map(|v| decode_timestamp(&v, "connection revoked"))
            .transpose()?,
        configuration,
        last_attempt: attempt
            .map(|v| decode_timestamp(&v, "connection last attempt"))
            .transpose()?,
        last_success: success
            .map(|v| decode_timestamp(&v, "connection last success"))
            .transpose()?,
        last_error,
    })
}
fn connection_status(s: ConnectionStatus) -> &'static str {
    match s {
        ConnectionStatus::Enabled => "enabled",
        ConnectionStatus::Paused => "paused",
        ConnectionStatus::Disconnected => "disconnected",
    }
}
fn policy_for(s: ConnectionStatus) -> &'static str {
    match s {
        ConnectionStatus::Enabled => "always",
        ConnectionStatus::Paused | ConnectionStatus::Disconnected => "paused",
    }
}

impl StoredEvent {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            occurred_at: row.get(1)?,
            captured_at: row.get(2)?,
            connector_id: row.get(3)?,
            source_reference: row.get(4)?,
            event_kind: row.get(5)?,
            payload_bytes: row.get(6)?,
            payload_media_type: row.get(7)?,
            payload_metadata_json: row.get(8)?,
            schema_version: row.get(9)?,
        })
    }

    fn into_event(self) -> Result<Event, StoreError> {
        Ok(Event::new(
            EventId::new(self.id),
            EventTimestamp::new(decode_timestamp(&self.occurred_at, "occurred_at")?),
            Source::new(
                ConnectorId::new(self.connector_id),
                SourceReference::new(self.source_reference),
            ),
            EventKind::new(self.event_kind),
            EventPayload::new(
                Arc::from(self.payload_bytes),
                self.payload_media_type.map(MediaType::new),
                decode_metadata(&self.payload_metadata_json)?,
            ),
            CaptureTimestamp::new(decode_timestamp(&self.captured_at, "captured_at")?),
            SchemaVersion::new(self.schema_version),
        ))
    }
}

struct StoredAnnotation {
    id: String,
    event_id: String,
    field: String,
    value_kind: String,
    value_text: String,
    assignment_source: String,
    assigned_at: Vec<u8>,
    supersedes_annotation_id: Option<String>,
    schema_version: String,
}

impl StoredAnnotation {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            event_id: row.get(1)?,
            field: row.get(2)?,
            value_kind: row.get(3)?,
            value_text: row.get(4)?,
            assignment_source: row.get(5)?,
            assigned_at: row.get(6)?,
            supersedes_annotation_id: row.get(7)?,
            schema_version: row.get(8)?,
        })
    }

    fn into_annotation(self) -> Result<Annotation, StoreError> {
        let value = match self.value_kind.as_str() {
            "context" => AnnotationValue::Context(ContextId::new(self.value_text)),
            "opaque" => AnnotationValue::opaque(self.value_text),
            _ => {
                return Err(StoreError::UnsupportedStoredData {
                    field: "annotation.value_kind",
                    value: self.value_kind,
                });
            }
        };
        Ok(Annotation::new(
            AnnotationId::new(self.id),
            EventId::new(self.event_id),
            AnnotationField::new(self.field),
            value,
            assignment_source_from_str(&self.assignment_source)?,
            AnnotationTimestamp::new(decode_timestamp(&self.assigned_at, "assigned_at")?),
            self.supersedes_annotation_id.map(AnnotationId::new),
            SchemaVersion::new(self.schema_version),
        ))
    }
}

fn assignment_source_to_str(source: AssignmentSource) -> &'static str {
    match source {
        AssignmentSource::Automatic => "automatic",
        AssignmentSource::User => "user",
        AssignmentSource::Imported => "imported",
        AssignmentSource::System => "system",
    }
}

fn assignment_source_from_str(value: &str) -> Result<AssignmentSource, StoreError> {
    match value {
        "automatic" => Ok(AssignmentSource::Automatic),
        "user" => Ok(AssignmentSource::User),
        "imported" => Ok(AssignmentSource::Imported),
        "system" => Ok(AssignmentSource::System),
        _ => Err(StoreError::UnsupportedStoredData {
            field: "annotation.assignment_source",
            value: value.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schomburg_core::{MetadataKey, MetadataValue};
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration, UNIX_EPOCH},
    };

    static DATABASE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryDatabase {
        path: PathBuf,
    }

    impl TemporaryDatabase {
        fn new() -> Self {
            let serial = DATABASE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "schomburg-store-test-{}-{serial}.sqlite",
                std::process::id()
            ));
            let _ = fs::remove_file(&path);
            Self { path }
        }
    }

    impl Drop for TemporaryDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn timestamp(seconds: u64) -> std::time::SystemTime {
        UNIX_EPOCH
            .checked_add(Duration::from_secs(seconds))
            .expect("test timestamp")
    }

    fn event(id: &str, occurred_at: std::time::SystemTime) -> Event {
        let mut metadata = BTreeMap::new();
        metadata.insert(MetadataKey::new("origin"), MetadataValue::new("test"));
        metadata.insert(
            MetadataKey::new("note"),
            MetadataValue::new("line one\nline two"),
        );
        Event::new(
            EventId::new(id),
            EventTimestamp::new(occurred_at),
            Source::new(
                ConnectorId::new("fixture.connector"),
                SourceReference::new("source-record-1"),
            ),
            EventKind::new("fixture.event"),
            EventPayload::new(
                Arc::from(vec![0, 1, 2, 255]),
                Some(MediaType::new("application/octet-stream")),
                metadata,
            ),
            CaptureTimestamp::new(timestamp(99)),
            SchemaVersion::new("event-v1"),
        )
    }

    fn annotation(
        id: &str,
        event_id: EventId,
        value: AnnotationValue,
        assigned_at: std::time::SystemTime,
        supersedes: Option<AnnotationId>,
    ) -> Annotation {
        annotation_with_field(id, event_id, "context", value, assigned_at, supersedes)
    }

    fn annotation_with_field(
        id: &str,
        event_id: EventId,
        field: &str,
        value: AnnotationValue,
        assigned_at: std::time::SystemTime,
        supersedes: Option<AnnotationId>,
    ) -> Annotation {
        Annotation::new(
            AnnotationId::new(id),
            event_id,
            AnnotationField::new(field),
            value,
            AssignmentSource::User,
            AnnotationTimestamp::new(assigned_at),
            supersedes,
            SchemaVersion::new("annotation-v1"),
        )
    }

    #[test]
    fn new_database_migration_succeeds() {
        Store::open_in_memory().expect("new database should migrate");
    }

    #[test]
    fn event_round_trip_preserves_every_field() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", timestamp(4));
        store.append_event(&event).expect("append event");

        assert_eq!(store.get_event(event.id()).expect("get event"), Some(event));
    }

    #[test]
    fn annotation_round_trip_preserves_every_field() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", timestamp(4));
        store.append_event(&event).expect("append event");
        let annotation = annotation(
            "annotation-1",
            event.id().clone(),
            AnnotationValue::Context(ContextId::new("context-1")),
            timestamp(5),
            None,
        );
        store
            .append_annotation(&annotation)
            .expect("append annotation");

        assert_eq!(
            store
                .get_annotation(annotation.id())
                .expect("get annotation"),
            Some(annotation)
        );
    }

    #[test]
    fn same_event_same_field_supersession_succeeds_without_rewriting_history() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", timestamp(4));
        store.append_event(&event).expect("append event");
        let earlier = annotation(
            "annotation-1",
            event.id().clone(),
            AnnotationValue::Context(ContextId::new("context-1")),
            timestamp(5),
            None,
        );
        store.append_annotation(&earlier).expect("append earlier");
        let later = annotation(
            "annotation-2",
            event.id().clone(),
            AnnotationValue::Context(ContextId::new("context-2")),
            timestamp(6),
            Some(earlier.id().clone()),
        );
        store.append_annotation(&later).expect("append later");

        assert_eq!(
            store.get_annotation(earlier.id()).expect("get earlier"),
            Some(earlier)
        );
        assert_eq!(
            store.get_annotation(later.id()).expect("get later"),
            Some(later)
        );
    }

    #[test]
    fn cross_event_supersession_fails_without_writing() {
        let store = Store::open_in_memory().expect("store");
        let first_event = event("event-1", timestamp(4));
        let second_event = event("event-2", timestamp(4));
        store
            .append_event(&first_event)
            .expect("append first event");
        store
            .append_event(&second_event)
            .expect("append second event");
        let earlier = annotation(
            "annotation-1",
            first_event.id().clone(),
            AnnotationValue::opaque("first"),
            timestamp(5),
            None,
        );
        store.append_annotation(&earlier).expect("append earlier");
        let invalid = annotation(
            "annotation-2",
            second_event.id().clone(),
            AnnotationValue::opaque("second"),
            timestamp(6),
            Some(earlier.id().clone()),
        );

        assert!(matches!(
            store.append_annotation(&invalid),
            Err(StoreError::SupersedesDifferentEvent {
                event_id,
                superseded_event_id,
            }) if event_id == *second_event.id() && superseded_event_id == *first_event.id()
        ));
        assert_eq!(
            store
                .list_annotations_for_event(first_event.id())
                .expect("list first event"),
            vec![earlier]
        );
        assert!(
            store
                .list_annotations_for_event(second_event.id())
                .expect("list second event")
                .is_empty()
        );
    }

    #[test]
    fn cross_field_supersession_fails_without_writing() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", timestamp(4));
        store.append_event(&event).expect("append event");
        let earlier = annotation(
            "annotation-1",
            event.id().clone(),
            AnnotationValue::opaque("first"),
            timestamp(5),
            None,
        );
        store.append_annotation(&earlier).expect("append earlier");
        let invalid = annotation_with_field(
            "annotation-2",
            event.id().clone(),
            "tag",
            AnnotationValue::opaque("second"),
            timestamp(6),
            Some(earlier.id().clone()),
        );

        assert!(matches!(
            store.append_annotation(&invalid),
            Err(StoreError::SupersedesDifferentField {
                field,
                superseded_field,
            }) if field.as_str() == "tag" && superseded_field.as_str() == "context"
        ));
        assert_eq!(
            store
                .list_annotations_for_event(event.id())
                .expect("list annotations"),
            vec![earlier]
        );
    }

    #[test]
    fn self_supersession_fails_without_writing() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", timestamp(4));
        store.append_event(&event).expect("append event");
        let invalid = annotation(
            "annotation-1",
            event.id().clone(),
            AnnotationValue::opaque("value"),
            timestamp(5),
            Some(AnnotationId::new("annotation-1")),
        );

        assert!(matches!(
            store.append_annotation(&invalid),
            Err(StoreError::SelfSupersession(id)) if id == *invalid.id()
        ));
        assert!(
            store
                .list_annotations_for_event(event.id())
                .expect("list annotations")
                .is_empty()
        );
    }

    #[test]
    fn multiple_annotations_for_one_event_remain_queryable() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", timestamp(4));
        store.append_event(&event).expect("append event");
        let first = annotation(
            "annotation-1",
            event.id().clone(),
            AnnotationValue::opaque("research"),
            timestamp(5),
            None,
        );
        let second = annotation(
            "annotation-2",
            event.id().clone(),
            AnnotationValue::opaque("important"),
            timestamp(6),
            None,
        );
        store.append_annotation(&first).expect("append first");
        store.append_annotation(&second).expect("append second");

        assert_eq!(
            store
                .list_annotations_for_event(event.id())
                .expect("list annotations"),
            vec![first, second]
        );
    }

    #[test]
    fn duplicate_event_id_is_rejected_without_overwriting() {
        let store = Store::open_in_memory().expect("store");
        let first = event("event-1", timestamp(4));
        let duplicate = event("event-1", timestamp(8));
        store.append_event(&first).expect("append first");

        assert!(matches!(
            store.append_event(&duplicate),
            Err(StoreError::DuplicateEventId(id)) if id == *first.id()
        ));
        assert_eq!(store.get_event(first.id()).expect("get event"), Some(first));
    }

    #[test]
    fn duplicate_annotation_id_is_rejected_without_overwriting() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", timestamp(4));
        store.append_event(&event).expect("append event");
        let first = annotation(
            "annotation-1",
            event.id().clone(),
            AnnotationValue::opaque("first"),
            timestamp(5),
            None,
        );
        let duplicate = annotation(
            "annotation-1",
            event.id().clone(),
            AnnotationValue::opaque("second"),
            timestamp(6),
            None,
        );
        store.append_annotation(&first).expect("append first");

        assert!(matches!(
            store.append_annotation(&duplicate),
            Err(StoreError::DuplicateAnnotationId(id)) if id == *first.id()
        ));
        assert_eq!(
            store.get_annotation(first.id()).expect("get annotation"),
            Some(first)
        );
    }

    #[test]
    fn events_list_chronologically_with_identifier_tie_breaker() {
        let store = Store::open_in_memory().expect("store");
        let later = event("event-z", timestamp(5));
        let first_at_same_time = event("event-a", timestamp(4));
        let second_at_same_time = event("event-b", timestamp(4));
        store.append_event(&later).expect("append later");
        store
            .append_event(&second_at_same_time)
            .expect("append second");
        store
            .append_event(&first_at_same_time)
            .expect("append first");

        assert_eq!(
            store.list_events().expect("list events"),
            vec![first_at_same_time, second_at_same_time, later]
        );
    }

    #[test]
    fn annotations_list_chronologically_with_identifier_tie_breaker() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", timestamp(4));
        store.append_event(&event).expect("append event");
        let later = annotation(
            "annotation-z",
            event.id().clone(),
            AnnotationValue::opaque("later"),
            timestamp(6),
            None,
        );
        let first_at_same_time = annotation(
            "annotation-a",
            event.id().clone(),
            AnnotationValue::opaque("first"),
            timestamp(5),
            None,
        );
        let second_at_same_time = annotation(
            "annotation-b",
            event.id().clone(),
            AnnotationValue::opaque("second"),
            timestamp(5),
            None,
        );
        store.append_annotation(&later).expect("append later");
        store
            .append_annotation(&second_at_same_time)
            .expect("append second");
        store
            .append_annotation(&first_at_same_time)
            .expect("append first");

        assert_eq!(
            store
                .list_annotations_for_event(event.id())
                .expect("list annotations"),
            vec![first_at_same_time, second_at_same_time, later]
        );
    }

    #[test]
    fn reopening_a_database_preserves_records() {
        let database = TemporaryDatabase::new();
        let event = event("event-1", timestamp(4));
        {
            let store = Store::open(&database.path).expect("open store");
            store.append_event(&event).expect("append event");
        }
        let reopened = Store::open(&database.path).expect("reopen store");

        assert_eq!(
            reopened.get_event(event.id()).expect("get event"),
            Some(event)
        );
    }

    #[test]
    fn context_values_round_trip_distinctly_from_opaque_values() {
        let store = Store::open_in_memory().expect("store");
        let event = event("event-1", timestamp(4));
        store.append_event(&event).expect("append event");
        let context = annotation(
            "annotation-context",
            event.id().clone(),
            AnnotationValue::Context(ContextId::new("same-value")),
            timestamp(5),
            None,
        );
        let opaque = annotation(
            "annotation-opaque",
            event.id().clone(),
            AnnotationValue::opaque("same-value"),
            timestamp(6),
            None,
        );
        store.append_annotation(&context).expect("append context");
        store.append_annotation(&opaque).expect("append opaque");

        assert_eq!(
            store.get_annotation(context.id()).expect("get context"),
            Some(context)
        );
        assert_eq!(
            store.get_annotation(opaque.id()).expect("get opaque"),
            Some(opaque)
        );
    }

    #[test]
    fn missing_target_and_supersedes_references_are_rejected() {
        let store = Store::open_in_memory().expect("store");
        let missing_target = annotation(
            "annotation-1",
            EventId::new("missing-event"),
            AnnotationValue::opaque("value"),
            timestamp(5),
            None,
        );
        assert!(matches!(
            store.append_annotation(&missing_target),
            Err(StoreError::MissingTargetEvent(id)) if id.as_str() == "missing-event"
        ));

        let event = event("event-1", timestamp(4));
        store.append_event(&event).expect("append event");
        let missing_supersedes = annotation(
            "annotation-2",
            event.id().clone(),
            AnnotationValue::opaque("value"),
            timestamp(5),
            Some(AnnotationId::new("missing-annotation")),
        );
        assert!(matches!(
            store.append_annotation(&missing_supersedes),
            Err(StoreError::MissingSupersedesAnnotation(id)) if id.as_str() == "missing-annotation"
        ));
    }
}
