//! Versioned local JSON-over-stdio transport for portable Schomburg shells.

use schomburg_agent::{parse_local_time, parse_schedule};
use schomburg_presenter::record_date_for;
use schomburg_service::{SchedulerStatus, SchomburgService, ServiceError};
use schomburg_store::{ConnectionId, DiscoveredSourceId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const PROTOCOL_VERSION: u8 = 1;

pub struct Host {
    service: SchomburgService,
    shutdown: bool,
}
#[derive(Deserialize)]
struct Request {
    protocol_version: u8,
    id: Value,
    command: String,
    #[serde(default)]
    params: Value,
}
#[derive(Serialize)]
struct Response {
    protocol_version: u8,
    id: Value,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ProtocolError>,
}
#[derive(Serialize)]
struct ProtocolError {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cause: Option<String>,
}

impl Host {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ServiceError> {
        Ok(Self {
            service: SchomburgService::open(path)?,
            shutdown: false,
        })
    }
    pub fn shutdown_requested(&self) -> bool {
        self.shutdown
    }
    pub fn handle_line(&mut self, line: &str) -> String {
        let request: Request = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                return response(
                    Value::Null,
                    Err((
                        "invalid_json",
                        "invalid JSON request".to_owned(),
                        Some(e.to_string()),
                    )),
                );
            }
        };
        if request.protocol_version != PROTOCOL_VERSION {
            return response(
                request.id,
                Err((
                    "invalid_request",
                    "unsupported protocol version".to_owned(),
                    None,
                )),
            );
        }
        let result = self.handle(request.command.as_str(), &request.params);
        response(request.id, result)
    }
    fn handle(
        &mut self,
        command: &str,
        params: &Value,
    ) -> Result<Value, (&'static str, String, Option<String>)> {
        let service = &self.service;
        match command {
            "status" => service.status().and_then(|status| service.scheduler_status().map(|scheduler| status_json(status, scheduler))).map_err(map_error),
            "update_record" => service.update_record(params.get("force").and_then(Value::as_bool).unwrap_or(false)).map(|r|json!({"imported":r.imported,"failed":r.failed,"affected_dates":r.dates_generated,"files_created":r.files_created,"files_updated":r.files_updated,"files_unchanged":r.files_unchanged})).map_err(map_error),
            "start_scheduler" => service.start_scheduler().map(|_|json!({"started":true})).map_err(map_error),
            "stop_scheduler" => service.stop_scheduler().map(|_|json!({"stopped":true})).map_err(map_error),
            "scheduler_status" => service.scheduler_status().map(|s|json!({"running":s.running,"lifecycle":format!("{:?}",s.lifecycle),"next_scheduled_run":time_json(s.next_run),"last_success":time_json(s.last_success),"last_error":s.last_error,"update_running":s.update_running})).map_err(map_error),
            "discover" => { let roots=params.get("scan_roots").and_then(Value::as_array).ok_or(("invalid_request","scan_roots is required".to_owned(),None))?.iter().map(|v|v.as_str().map(PathBuf::from).ok_or(("invalid_request","scan_roots must contain strings".to_owned(),None))).collect::<Result<Vec<_>,_>>()?; service.discover(&roots).map(|count|json!({"discovered":count})).map_err(map_error) }
            "list_sources" => service.list_sources().map(|sources|json!(sources.into_iter().map(|s|json!({"id":s.id.as_str(),"connector":s.connector_id.as_str(),"display_name":s.display_name,"reference":s.local_reference,"status":format!("{:?}",s.status)})).collect::<Vec<_>>())).map_err(map_error),
            "connect_source" => service.connect(&source_id(params)?).map(|id|json!({"connection_id":id.as_str()})).map_err(map_error),
            "decline_source" => service.decline(&source_id(params)?).map(|_|json!({})).map_err(map_error),
            "list_connections" => service.list_connections().map(|items|json!(items.into_iter().map(|c|json!({"id":c.id.as_str(),"source_id":c.source_id.as_str(),"connector":c.connector_id.as_str(),"status":format!("{:?}",c.status)})).collect::<Vec<_>>())).map_err(map_error),
            "pause_connection" => service.pause_connection(&connection_id(params)?).map(|_|json!({})).map_err(map_error),
            "resume_connection" => service.resume_connection(&connection_id(params)?).map(|_|json!({})).map_err(map_error),
            "disconnect_connection" => service.disconnect_connection(&connection_id(params)?).map(|_|json!({})).map_err(map_error),
            "set_monitoring" => service.set_monitoring_enabled(params.get("enabled").and_then(Value::as_bool).ok_or(("invalid_request","enabled is required".to_owned(),None))?).map(|_|json!({})).map_err(map_error),
            "set_record_folder" => service.set_record_folder(string_param(params,"folder")?).map(|_|json!({})).map_err(map_error),
            "set_schedule" => { let mut c=service.get_schedule().map_err(map_error)?; c.time=parse_local_time(string_param(params,"time")?).map_err(|e|("invalid_request",e.to_string(),None))?; c.schedule=parse_schedule(string_param(params,"days")?).map_err(|e|("invalid_request",e.to_string(),None))?; service.set_schedule(c).map(|_|json!({})).map_err(map_error) }
            "get_schedule" => service.get_schedule().map(|c|json!({"monitoring":format!("{:?}",c.monitoring),"schedule":format!("{:?}",c.schedule),"time":format!("{:02}:{:02}",c.time.hour,c.time.minute),"next_scheduled_run":time_json(c.next_run)})).map_err(map_error),
            "shutdown" => { self.shutdown=true; Ok(json!({"shutdown":true})) }
            _ => Err(("unsupported_command","unsupported command".to_owned(),None)),
        }
    }
}
fn response(id: Value, result: Result<Value, (&'static str, String, Option<String>)>) -> String {
    match result {
        Ok(value) => serde_json::to_string(&Response {
            protocol_version: PROTOCOL_VERSION,
            id,
            ok: true,
            result: Some(value),
            error: None,
        })
        .expect("serializable"),
        Err((code, message, cause)) => serde_json::to_string(&Response {
            protocol_version: PROTOCOL_VERSION,
            id,
            ok: false,
            result: None,
            error: Some(ProtocolError {
                code,
                message,
                cause,
            }),
        })
        .expect("serializable"),
    }
}
fn string_param<'a>(
    params: &'a Value,
    key: &str,
) -> Result<&'a str, (&'static str, String, Option<String>)> {
    params.get(key).and_then(Value::as_str).ok_or((
        "invalid_request",
        format!("{key} is required"),
        None,
    ))
}
fn source_id(params: &Value) -> Result<DiscoveredSourceId, (&'static str, String, Option<String>)> {
    Ok(DiscoveredSourceId::new(string_param(params, "source_id")?))
}
fn connection_id(params: &Value) -> Result<ConnectionId, (&'static str, String, Option<String>)> {
    Ok(ConnectionId::new(string_param(params, "connection_id")?))
}
fn time_json(value: Option<SystemTime>) -> Option<u64> {
    value.and_then(|v| v.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs()))
}
fn status_json(status: schomburg_service::ServiceStatus, scheduler: SchedulerStatus) -> Value {
    let folder = status.configuration.record_folder.clone();
    let today = folder.as_deref().and_then(|folder| {
        record_date_for(SystemTime::now()).ok().map(|date| {
            let year = &date.as_str()[0..4];
            let month = &date.as_str()[5..7];
            Path::new(folder)
                .join(year)
                .join(month)
                .join(format!("{}.md", date.as_str()))
                .display()
                .to_string()
        })
    });
    let attention = folder.is_none()
        || status.awaiting_consent > 0
        || status.manual_update.last_error.is_some()
        || status.scheduled_reconciliation.last_error.is_some();
    json!({"monitoring":format!("{:?}",status.configuration.monitoring),"scheduler_lifecycle": format!("{:?}",scheduler.lifecycle),"update_running":status.update_running,"record_folder":folder,"today_record_path":today,"connected_sources":status.connected_sources,"awaiting_consent":status.awaiting_consent,"next_scheduled_run":time_json(status.scheduled_reconciliation.next_scheduled_run.or(status.configuration.next_run)),"manual_update":{"last_success":time_json(status.manual_update.last_success),"last_error":status.manual_update.last_error,"counts":{"imported":status.manual_update.counts.imported,"duplicates":status.manual_update.counts.duplicates,"rejected":status.manual_update.counts.rejected,"failed":status.manual_update.counts.failed}},"scheduled_reconciliation":{"last_success":time_json(status.scheduled_reconciliation.last_success),"last_error":status.scheduled_reconciliation.last_error,"last_reconciled_local_date":status.scheduled_reconciliation.last_reconciled_local_date,"counts":{"imported":status.scheduled_reconciliation.counts.imported,"duplicates":status.scheduled_reconciliation.counts.duplicates,"rejected":status.scheduled_reconciliation.counts.rejected,"failed":status.scheduled_reconciliation.counts.failed}},"attention_required":attention})
}
fn map_error(error: ServiceError) -> (&'static str, String, Option<String>) {
    match error {
        ServiceError::UpdateAlreadyRunning => (
            "update_already_running",
            "Update Record already running".to_owned(),
            None,
        ),
        ServiceError::SchedulerAlreadyRunning => (
            "scheduler_already_running",
            "scheduler already running".to_owned(),
            None,
        ),
        ServiceError::SchedulerNotRunning => (
            "scheduler_not_running",
            "scheduler is not running".to_owned(),
            None,
        ),
        ServiceError::UnknownSource => ("unknown_source", "unknown source".to_owned(), None),
        ServiceError::UnknownConnection => {
            ("unknown_connection", "unknown connection".to_owned(), None)
        }
        ServiceError::Database(_) => (
            "service_unavailable",
            "service unavailable".to_owned(),
            None,
        ),
        ServiceError::Agent(message) if message.contains("no Record Folder") => (
            "missing_record_folder",
            "no Record Folder configured".to_owned(),
            None,
        ),
        ServiceError::Agent(message) if message.contains("invalid connection transition") => (
            "invalid_transition",
            "invalid connection transition".to_owned(),
            None,
        ),
        _ => (
            "operation_failed",
            "service operation failed".to_owned(),
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature, Time};
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SERIAL: AtomicUsize = AtomicUsize::new(0);
    fn directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "schomburg-host-{label}-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("directory");
        path
    }
    fn request(host: &mut Host, id: u64, command: &str, params: Value) -> Value {
        serde_json::from_str(&host.handle_line(
            &json!({"protocol_version":1,"id":id,"command":command,"params":params}).to_string(),
        ))
        .expect("response")
    }
    fn repository(root: &Path) -> PathBuf {
        let path = root.join("repository");
        let repo = Repository::init(&path).expect("repository");
        let tree_id = repo
            .treebuilder(None)
            .expect("tree")
            .write()
            .expect("tree id");
        let tree = repo.find_tree(tree_id).expect("tree");
        let signature = Signature::new("Test", "test@example.com", &Time::new(1_700_000_000, 0))
            .expect("signature");
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "host evidence",
            &tree,
            &[],
        )
        .expect("commit");
        path
    }
    #[test]
    fn protocol_round_trips_ids_and_errors() {
        let path = std::env::temp_dir().join(format!("host-{}.sqlite3", std::process::id()));
        let _ = fs::remove_file(&path);
        let mut host = Host::open(&path).expect("open");
        let response: Value = serde_json::from_str(
            &host.handle_line(r#"{"protocol_version":1,"id":"a","command":"status"}"#),
        )
        .expect("json");
        assert_eq!(response["id"], "a");
        assert!(response["ok"].as_bool().expect("bool"));
        let invalid: Value = serde_json::from_str(&host.handle_line("nope")).expect("json");
        assert_eq!(invalid["error"]["code"], "invalid_json");
        let unsupported: Value = serde_json::from_str(
            &host.handle_line(r#"{"protocol_version":1,"id":2,"command":"nope"}"#),
        )
        .expect("json");
        assert_eq!(unsupported["error"]["code"], "unsupported_command");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn configuration_status_and_error_mapping_round_trip_through_host() {
        let root = directory("configuration");
        let database = root.join("host.sqlite3");
        let folder = root.join("records");
        let mut host = Host::open(&database).expect("new database");
        let status = request(&mut host, 1, "status", json!({}));
        assert_eq!(status["protocol_version"], 1);
        assert_eq!(
            status["result"]["manual_update"]["last_success"],
            Value::Null
        );
        assert_eq!(
            status["result"]["scheduled_reconciliation"]["last_success"],
            Value::Null
        );
        assert!(
            request(&mut host, 2, "set_monitoring", json!({"enabled":true}))["ok"]
                .as_bool()
                .expect("ok")
        );
        assert!(
            request(&mut host, 3, "set_record_folder", json!({"folder":folder}))["ok"]
                .as_bool()
                .expect("ok")
        );
        assert!(
            request(
                &mut host,
                4,
                "set_schedule",
                json!({"time":"18:30","days":"weekdays"})
            )["ok"]
                .as_bool()
                .expect("ok")
        );
        let schedule = request(&mut host, 5, "get_schedule", json!({}));
        assert_eq!(schedule["result"]["time"], "18:30");
        let reopened = Host::open(&database).expect("existing database");
        drop(reopened);
        let updated = request(&mut host, 6, "update_record", json!({}));
        assert!(updated["ok"].as_bool().expect("ok"));
        let status = request(&mut host, 7, "status", json!({}));
        assert_eq!(
            status["result"]["record_folder"],
            folder.display().to_string()
        );
        assert!(
            status["result"]["today_record_path"]
                .as_str()
                .expect("path")
                .ends_with(".md")
        );
        let unknown = request(
            &mut host,
            8,
            "connect_source",
            json!({"source_id":"missing"}),
        );
        assert_eq!(unknown["error"]["code"], "unknown_source");
        let missing = request(
            &mut Host::open(root.join("missing.sqlite3")).expect("host"),
            9,
            "update_record",
            json!({}),
        );
        assert_eq!(missing["error"]["code"], "missing_record_folder");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discovery_connection_scheduler_and_shutdown_use_one_protocol_instance() {
        let root = directory("lifecycle");
        let database = root.join("host.sqlite3");
        let repo = repository(&root);
        let mut host = Host::open(&database).expect("host");
        let discovered = request(&mut host, 1, "discover", json!({"scan_roots":[root]}));
        assert_eq!(discovered["result"]["discovered"], 1);
        let sources = request(&mut host, 2, "list_sources", json!({}));
        let source = sources["result"][0]["id"]
            .as_str()
            .expect("source")
            .to_owned();
        let connection = request(&mut host, 3, "connect_source", json!({"source_id":source}));
        let id = connection["result"]["connection_id"]
            .as_str()
            .expect("connection")
            .to_owned();
        assert!(
            request(
                &mut host,
                4,
                "pause_connection",
                json!({"connection_id":id})
            )["ok"]
                .as_bool()
                .expect("ok")
        );
        let invalid = request(
            &mut host,
            5,
            "pause_connection",
            json!({"connection_id":id}),
        );
        assert_eq!(invalid["error"]["code"], "invalid_transition");
        assert!(
            request(
                &mut host,
                6,
                "resume_connection",
                json!({"connection_id":id})
            )["ok"]
                .as_bool()
                .expect("ok")
        );
        assert!(
            request(&mut host, 7, "start_scheduler", json!({}))["ok"]
                .as_bool()
                .expect("ok")
        );
        assert_eq!(
            request(&mut host, 8, "start_scheduler", json!({}))["error"]["code"],
            "scheduler_already_running"
        );
        assert!(
            request(&mut host, 9, "stop_scheduler", json!({}))["ok"]
                .as_bool()
                .expect("ok")
        );
        assert!(
            request(&mut host, 10, "shutdown", json!({}))["result"]["shutdown"]
                .as_bool()
                .expect("shutdown")
        );
        assert!(host.shutdown_requested());
        assert!(repo.exists());
        let _ = fs::remove_dir_all(root);
    }
}
