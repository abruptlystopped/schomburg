//! Mutable operational records for discovery and approved connections.

use schomburg_core::ConnectorId;
use std::{collections::BTreeMap, time::SystemTime};

macro_rules! id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub struct $name(pub(crate) String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
id!(DiscoveredSourceId);
id!(ConnectionId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryStatus {
    AwaitingDecision,
    Declined,
    Connected,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionStatus {
    Enabled,
    Paused,
    Disconnected,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonitoringPolicy {
    Always,
    Paused,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlobalMonitoringState {
    Enabled,
    Paused,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationState {
    Idle,
    Running,
    Succeeded,
    Failed,
    Paused,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationSchedule {
    Daily,
    Weekdays,
    Selected(u8),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalReconciliationTime {
    pub hour: u8,
    pub minute: u8,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReconciliationCounts {
    pub imported: u64,
    pub duplicates: u64,
    pub rejected: u64,
    pub failed: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationConfiguration {
    pub monitoring: GlobalMonitoringState,
    pub record_folder: Option<String>,
    pub schedule: ReconciliationSchedule,
    pub time: LocalReconciliationTime,
    pub last_attempt: Option<SystemTime>,
    pub last_success: Option<SystemTime>,
    pub next_run: Option<SystemTime>,
    pub last_error: Option<String>,
    pub counts: ReconciliationCounts,
    pub state: ReconciliationState,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualUpdateStatus {
    pub last_attempt: Option<SystemTime>,
    pub last_success: Option<SystemTime>,
    pub last_error: Option<String>,
    pub counts: ReconciliationCounts,
    pub state: ReconciliationState,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduledReconciliationStatus {
    pub last_attempt: Option<SystemTime>,
    pub last_success: Option<SystemTime>,
    pub last_error: Option<String>,
    pub counts: ReconciliationCounts,
    pub state: ReconciliationState,
    pub last_reconciled_local_date: Option<String>,
    pub next_scheduled_run: Option<SystemTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredSource {
    pub id: DiscoveredSourceId,
    pub connector_id: ConnectorId,
    pub source_identity: String,
    pub display_name: String,
    pub local_reference: Option<String>,
    pub first_discovered: SystemTime,
    pub last_seen: SystemTime,
    pub status: DiscoveryStatus,
    pub metadata: BTreeMap<String, String>,
    pub configuration: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionRecord {
    pub id: ConnectionId,
    pub source_id: DiscoveredSourceId,
    pub connector_id: ConnectorId,
    pub status: ConnectionStatus,
    pub policy: MonitoringPolicy,
    pub approved_at: SystemTime,
    pub revoked_at: Option<SystemTime>,
    pub configuration: String,
    pub last_attempt: Option<SystemTime>,
    pub last_success: Option<SystemTime>,
    pub last_error: Option<String>,
}
