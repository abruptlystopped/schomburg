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
