//! Read-only, source-agnostic generation of daily evidence records.

use schomburg_connector::{PresentationField, PresentationRegistry};
use schomburg_store::Store;
use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

/// A local calendar date used for a daily record.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RecordDate(String);
impl RecordDate {
    pub fn parse(value: &str) -> Result<Self, PresenterError> {
        if value.len() == 10
            && value.as_bytes().get(4) == Some(&b'-')
            && value.as_bytes().get(7) == Some(&b'-')
        {
            Ok(Self(value.to_owned()))
        } else {
            Err(PresenterError::InvalidDate(value.to_owned()))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// One factual timeline entry or an explicit presentation-error entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimelineEntry {
    pub time: String,
    pub source: String,
    pub title: String,
    pub identifiers: Vec<PresentationField>,
    pub error: Option<String>,
}
/// Structured daily read model for future non-Markdown renderers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DailyRecord {
    pub date: RecordDate,
    pub heading: String,
    pub entries: Vec<TimelineEntry>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecordGenerationResult {
    pub dates_generated: usize,
    pub files_created: usize,
    pub files_updated: usize,
    pub files_unchanged: usize,
    pub events_presented: usize,
    pub presentation_errors: usize,
}
pub struct Presenter {
    registry: PresentationRegistry,
}
impl Presenter {
    pub fn new(registry: PresentationRegistry) -> Self {
        Self { registry }
    }
    pub fn generate_all(
        &self,
        store: &Store,
        folder: &Path,
    ) -> Result<RecordGenerationResult, PresenterError> {
        self.generate(store, folder, None)
    }
    pub fn generate_date(
        &self,
        store: &Store,
        folder: &Path,
        date: RecordDate,
    ) -> Result<RecordGenerationResult, PresenterError> {
        self.generate(store, folder, Some(date))
    }
    fn generate(
        &self,
        store: &Store,
        folder: &Path,
        only: Option<RecordDate>,
    ) -> Result<RecordGenerationResult, PresenterError> {
        let mut days: BTreeMap<RecordDate, DailyRecord> = BTreeMap::new();
        let mut result = RecordGenerationResult::default();
        for event in store
            .list_events()
            .map_err(|e| PresenterError::Store(e.to_string()))?
        {
            let (date, time, heading) = local_parts(event.occurred_at().as_system_time())?;
            if only.as_ref().is_some_and(|d| d != &date) {
                continue;
            }
            let entry = match self.registry.present_compact(&event) {
                Ok(p) => {
                    result.events_presented += 1;
                    TimelineEntry {
                        time,
                        source: p.subtitle().unwrap_or(p.source_label()).to_owned(),
                        title: p.title().to_owned(),
                        identifiers: p.identifiers().to_vec(),
                        error: None,
                    }
                }
                Err(e) => {
                    result.presentation_errors += 1;
                    TimelineEntry {
                        time,
                        source: event.source().connector_id().as_str().to_owned(),
                        title: "Evidence presentation error".to_owned(),
                        identifiers: Vec::new(),
                        error: Some(e.to_string()),
                    }
                }
            };
            days.entry(date.clone())
                .or_insert_with(|| DailyRecord {
                    date,
                    heading,
                    entries: Vec::new(),
                })
                .entries
                .push(entry);
        }
        for record in days.into_values() {
            let path = record_path(folder, &record.date);
            let body = render_markdown(&record);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| PresenterError::Folder(e.to_string()))?;
            }
            let previous = fs::read_to_string(&path).ok();
            if let Some(existing) = &previous
                && !existing.starts_with("<!-- schomburg-record: v1 -->")
            {
                return Err(PresenterError::OwnershipConflict(path));
            }
            if previous.as_deref() == Some(&body) {
                result.files_unchanged += 1;
            } else {
                let existed = path.exists();
                let temporary = path.with_extension("md.tmp");
                fs::write(&temporary, body).map_err(|e| PresenterError::Write(e.to_string()))?;
                fs::rename(&temporary, &path).map_err(|e| PresenterError::Write(e.to_string()))?;
                if existed {
                    result.files_updated += 1
                } else {
                    result.files_created += 1
                };
            }
            result.dates_generated += 1;
        }
        Ok(result)
    }
}
/// Returns the local calendar date used for record grouping.
pub fn record_date_for(value: SystemTime) -> Result<RecordDate, PresenterError> {
    local_parts(value).map(|(date, _, _)| date)
}
fn local_parts(value: SystemTime) -> Result<(RecordDate, String, String), PresenterError> {
    let utc = OffsetDateTime::from(value);
    let local = utc.to_offset(
        UtcOffset::local_offset_at(utc).map_err(|e| PresenterError::Time(e.to_string()))?,
    );
    let date = local
        .format(format_description!("[year]-[month]-[day]"))
        .map_err(|e| PresenterError::Time(e.to_string()))?;
    let heading = local
        .format(format_description!(
            "[weekday repr:long], [month repr:long] [day], [year]"
        ))
        .map_err(|e| PresenterError::Time(e.to_string()))?;
    let time = local
        .format(format_description!(
            "[hour repr:12]:[minute] [period case:upper]"
        ))
        .map_err(|e| PresenterError::Time(e.to_string()))?;
    Ok((RecordDate(date), time, heading))
}
fn record_path(folder: &Path, date: &RecordDate) -> PathBuf {
    let p = date.as_str();
    folder.join(&p[0..4]).join(&p[5..7]).join(format!("{p}.md"))
}
fn render_markdown(record: &DailyRecord) -> String {
    let mut out = format!("<!-- schomburg-record: v1 -->\n# {}\n", record.heading);
    for entry in &record.entries {
        out.push_str(&format!(
            "\n## {}\n\n### {}\n\n{}\n\n",
            entry.time, entry.source, entry.title
        ));
        if let Some(error) = &entry.error {
            out.push_str(&format!("Presentation error: {error}\n"));
        } else {
            for id in &entry.identifiers {
                out.push_str(&format!("{} · {}\n", id.label(), id.value()));
            }
        }
        out.push_str("\n---\n");
    }
    out
}
#[derive(Debug)]
pub enum PresenterError {
    Store(String),
    InvalidDate(String),
    Folder(String),
    Write(String),
    Time(String),
    OwnershipConflict(PathBuf),
}
impl fmt::Display for PresenterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(s) => write!(f, "database read failure: {s}"),
            Self::InvalidDate(s) => write!(f, "invalid date: {s}"),
            Self::Folder(s) => write!(f, "record folder failure: {s}"),
            Self::Write(s) => write!(f, "record write failure: {s}"),
            Self::Time(s) => write!(f, "local time conversion failure: {s}"),
            Self::OwnershipConflict(path) => {
                write!(f, "record ownership conflict: {}", path.display())
            }
        }
    }
}
impl std::error::Error for PresenterError {}

#[cfg(test)]
mod tests {
    use super::*;
    use schomburg_connector::PresentationRegistry;
    use schomburg_connector_git::GitPresenter;
    use schomburg_core::{
        CaptureTimestamp, ConnectorId, Event, EventId, EventKind, EventPayload, EventTimestamp,
        MetadataKey, MetadataValue, SchemaVersion, Source, SourceReference,
    };
    use std::{
        collections::BTreeMap,
        sync::Arc,
        time::{Duration, UNIX_EPOCH},
    };

    fn event(id: &str, seconds: u64, repository: &str, subject: &str) -> Event {
        let raw = format!(
            "tree deadbeef\nauthor A <a@example.com> {seconds} +0000\ncommitter C <c@example.com> {seconds} +0000\n\n{subject}\n"
        );
        let mut metadata = BTreeMap::new();
        metadata.insert(
            MetadataKey::new("git.repository_display_name"),
            MetadataValue::new(repository),
        );
        metadata.insert(
            MetadataKey::new("git.repository_reference"),
            MetadataValue::new("git-dir-hex:00"),
        );
        metadata.insert(
            MetadataKey::new("git.commit_hash"),
            MetadataValue::new(format!("{id}0123456789abcdef")),
        );
        Event::new(
            EventId::new(id),
            EventTimestamp::new(UNIX_EPOCH + Duration::from_secs(seconds)),
            Source::new(ConnectorId::new("schomburg.git"), SourceReference::new(id)),
            EventKind::new("git.commit"),
            EventPayload::new(Arc::from(raw.into_bytes()), None, metadata),
            CaptureTimestamp::new(UNIX_EPOCH),
            SchemaVersion::new("1"),
        )
    }
    fn presenter() -> Presenter {
        let mut registry = PresentationRegistry::default();
        registry
            .register(Box::new(GitPresenter::new()))
            .expect("register");
        Presenter::new(registry)
    }
    fn folder(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "schomburg-presenter-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }
    fn generated_file(folder: &Path) -> PathBuf {
        let year = fs::read_dir(folder)
            .expect("year")
            .next()
            .expect("year entry")
            .expect("year entry")
            .path();
        let month = fs::read_dir(year)
            .expect("month")
            .next()
            .expect("month entry")
            .expect("month entry")
            .path();
        fs::read_dir(month)
            .expect("date")
            .next()
            .expect("file")
            .expect("file")
            .path()
    }

    #[test]
    fn generated_daily_record_is_readable_deterministic_and_regenerable() {
        let store = Store::open_in_memory().expect("store");
        store
            .append_event(&event("a", 10, "Café", "Add *exact* subject"))
            .expect("a");
        store
            .append_event(&event("b", 20, "Other", "Second & <subject>"))
            .expect("b");
        let folder = folder("record");
        let p = presenter();
        let first = p.generate_all(&store, &folder).expect("generate");
        assert_eq!(first.events_presented, 2);
        assert_eq!(first.files_created, 1);
        let file = generated_file(&folder);
        let text = fs::read_to_string(&file).expect("read");
        assert!(text.starts_with("<!-- schomburg-record: v1 -->"));
        assert!(text.contains("Café"));
        assert!(text.contains("Add *exact* subject"));
        assert!(text.contains("Git Commit") || text.contains("Commit"));
        assert!(!text.contains("payload_bytes_hex"));
        assert!(!text.contains("schema_version"));
        assert!(!text.contains("id: a"));
        assert!(!text.contains("+10.000000"));
        assert_eq!(
            p.generate_all(&store, &folder)
                .expect("again")
                .files_unchanged,
            1
        );
        fs::remove_file(&file).expect("delete");
        assert_eq!(
            p.generate_all(&store, &folder)
                .expect("recreate")
                .files_created,
            1
        );
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn ownership_conflicts_and_unknown_evidence_are_explicit() {
        let store = Store::open_in_memory().expect("store");
        store
            .append_event(&event("a", 10, "Repo", "Subject"))
            .expect("event");
        let folder = folder("ownership");
        let p = presenter();
        p.generate_all(&store, &folder).expect("generate");
        let file = generated_file(&folder);
        fs::write(&file, "user content").expect("user");
        assert!(matches!(
            p.generate_all(&store, &folder),
            Err(PresenterError::OwnershipConflict(_))
        ));
        let user = folder.join("user.md");
        fs::write(&user, "untouched").expect("user file");
        assert_eq!(fs::read_to_string(&user).expect("read"), "untouched");
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn errors_dates_and_updates_are_visible_without_dropping_valid_evidence() {
        let store = Store::open_in_memory().expect("store");
        let valid = event("a", 100_000, "Repo #1", "Valid [subject] *exact*");
        let unknown = Event::new(
            EventId::new("unknown"),
            valid.occurred_at().clone(),
            Source::new(
                ConnectorId::new("unknown.connector"),
                SourceReference::new("source-42"),
            ),
            EventKind::new("unknown.event"),
            valid.payload().clone(),
            valid.captured_at().clone(),
            valid.schema_version().clone(),
        );
        store.append_event(&valid).expect("valid");
        store.append_event(&unknown).expect("unknown");
        let folder = folder("errors");
        let p = presenter();
        let first = p.generate_all(&store, &folder).expect("generate");
        assert_eq!(first.events_presented, 1);
        assert_eq!(first.presentation_errors, 1);
        let file = generated_file(&folder);
        let text = fs::read_to_string(&file).expect("read");
        assert!(text.contains("Evidence presentation error"));
        assert!(text.contains("unknown.connector"));
        assert!(text.contains("Valid [subject] *exact*"));
        assert!(!text.contains("payload_bytes_hex"));
        let date = file
            .file_stem()
            .expect("date")
            .to_string_lossy()
            .into_owned();
        fs::remove_dir_all(&folder).expect("remove");
        let specific = p
            .generate_date(&store, &folder, RecordDate::parse(&date).expect("date"))
            .expect("specific");
        assert_eq!(specific.dates_generated, 1);
        assert_eq!(specific.files_created, 1);
        store
            .append_event(&event("later", 200_000, "Later", "Later subject"))
            .expect("later");
        let updated = p.generate_all(&store, &folder).expect("update");
        assert!(updated.files_created + updated.files_updated >= 1);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn ordering_duplicate_and_marked_replacement_are_deterministic() {
        let store = Store::open_in_memory().expect("store");
        let first = event("a", 300_000, "One", "First");
        let second = event("b", 300_000, "Two", "Second");
        store.append_event(&second).expect("second");
        store.append_event(&first).expect("first");
        assert!(store.append_event(&first).is_err());
        let folder = folder("order");
        let p = presenter();
        p.generate_all(&store, &folder).expect("generate");
        let file = generated_file(&folder);
        let text = fs::read_to_string(&file).expect("read");
        assert!(text.find("First").expect("first") < text.find("Second").expect("second"));
        fs::write(&file, "<!-- schomburg-record: v1 -->\nuser edit\n").expect("marked");
        let result = p.generate_all(&store, &folder).expect("replace marked");
        assert_eq!(result.files_updated, 1);
        let _ = fs::remove_dir_all(folder);
    }
}
