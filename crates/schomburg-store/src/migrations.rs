use crate::StoreError;
use rusqlite::{Connection, OptionalExtension};

const MIGRATIONS: &[(i64, &str)] = &[(
    1,
    "
    CREATE TABLE events (
        id TEXT PRIMARY KEY NOT NULL,
        occurred_at BLOB NOT NULL CHECK (length(occurred_at) = 13),
        captured_at BLOB NOT NULL CHECK (length(captured_at) = 13),
        connector_id TEXT NOT NULL,
        source_reference TEXT NOT NULL,
        event_kind TEXT NOT NULL,
        payload_bytes BLOB NOT NULL,
        payload_media_type TEXT,
        payload_metadata_json TEXT NOT NULL,
        schema_version TEXT NOT NULL
    );

    CREATE TABLE annotations (
        id TEXT PRIMARY KEY NOT NULL,
        event_id TEXT NOT NULL REFERENCES events(id) ON DELETE RESTRICT,
        field TEXT NOT NULL,
        value_kind TEXT NOT NULL CHECK (value_kind IN ('context', 'opaque')),
        value_text TEXT NOT NULL,
        assignment_source TEXT NOT NULL CHECK (
            assignment_source IN ('automatic', 'user', 'imported', 'system')
        ),
        assigned_at BLOB NOT NULL CHECK (length(assigned_at) = 13),
        supersedes_annotation_id TEXT REFERENCES annotations(id) ON DELETE RESTRICT,
        schema_version TEXT NOT NULL
    );

    CREATE INDEX idx_events_occurred_at_id ON events(occurred_at, id);
    CREATE INDEX idx_annotations_event_assigned_at_id
        ON annotations(event_id, assigned_at, id);
    CREATE INDEX idx_annotations_supersedes
        ON annotations(supersedes_annotation_id);
    ",
), (2, "
    CREATE TABLE discovered_sources (
      id TEXT PRIMARY KEY NOT NULL, connector_id TEXT NOT NULL, source_identity TEXT NOT NULL,
      display_name TEXT NOT NULL, local_reference TEXT, first_discovered BLOB NOT NULL,
      last_seen BLOB NOT NULL, status TEXT NOT NULL, metadata_json TEXT NOT NULL, configuration TEXT NOT NULL,
      UNIQUE(connector_id, source_identity)
    );
    CREATE TABLE connections (
      id TEXT PRIMARY KEY NOT NULL, source_id TEXT NOT NULL REFERENCES discovered_sources(id), connector_id TEXT NOT NULL,
      status TEXT NOT NULL, policy TEXT NOT NULL, approved_at BLOB NOT NULL, revoked_at BLOB,
      configuration TEXT NOT NULL, last_attempt BLOB, last_success BLOB, last_error TEXT
    );
    CREATE INDEX idx_connections_status ON connections(status, policy);
"), (3, "
 CREATE TABLE reconciliation_configuration (
 id INTEGER PRIMARY KEY CHECK (id = 1), monitoring TEXT NOT NULL, record_folder TEXT,
 schedule_kind TEXT NOT NULL, schedule_days INTEGER NOT NULL, local_hour INTEGER NOT NULL, local_minute INTEGER NOT NULL,
 last_attempt BLOB, last_success BLOB, next_run BLOB, last_error TEXT,
 imported INTEGER NOT NULL, duplicates INTEGER NOT NULL, rejected INTEGER NOT NULL, failed INTEGER NOT NULL, state TEXT NOT NULL
 );
 INSERT INTO reconciliation_configuration VALUES (1,'paused',NULL,'daily',0,9,0,NULL,NULL,NULL,NULL,0,0,0,0,'paused');
")];

pub(crate) fn apply(connection: &mut Connection) -> Result<(), StoreError> {
    connection
        .execute_batch(
            "
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY NOT NULL
            );
            ",
        )
        .map_err(StoreError::database)?;

    for (version, sql) in MIGRATIONS {
        let installed = connection
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE version = ?1",
                [version],
                |_| Ok(()),
            )
            .optional()
            .map_err(StoreError::database)?
            .is_some();

        if !installed {
            let transaction = connection.transaction().map_err(StoreError::database)?;
            transaction
                .execute_batch(sql)
                .map_err(StoreError::database)?;
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version) VALUES (?1)",
                    [version],
                )
                .map_err(StoreError::database)?;
            transaction.commit().map_err(StoreError::database)?;
        }
    }

    Ok(())
}
