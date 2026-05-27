use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================
// Error Types
// ============================================================

#[derive(Debug, Error)]
pub enum CloudflareDnsError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    #[error("Cloudflare API error (status {status}): {message}")]
    ApiError { status: StatusCode, message: String },

    #[error("Record not found: {0}")]
    NotFound(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

// ============================================================
// Cloudflare API Response Wrappers
// ============================================================

#[derive(Debug, Deserialize)]
pub struct CloudflareResponse<T> {
    pub success: bool,
    pub errors: Vec<CloudflareApiError>,
    pub messages: Vec<CloudflareMessage>,
    pub result: Option<T>,
}

#[derive(Debug, Deserialize)]
pub struct CloudflareListResponse<T> {
    pub success: bool,
    pub errors: Vec<CloudflareApiError>,
    pub messages: Vec<CloudflareMessage>,
    pub result: Option<Vec<T>>,
    pub result_info: Option<ResultInfo>,
}

#[derive(Debug, Deserialize)]
pub struct CloudflareApiError {
    pub code: u32,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct CloudflareMessage {
    pub code: u32,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ResultInfo {
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
    pub count: u32,
    pub total_count: u32,
}

// ============================================================
// DNS Record Types
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum DnsRecordType {
    A,
    Aaaa,
    Caa,
    Cert,
    Cname,
    Dnskey,
    Ds,
    Https,
    Loc,
    Mx,
    Naptr,
    Ns,
    Ptr,
    Smimea,
    Srv,
    Sshfp,
    Svcb,
    Tlsa,
    Txt,
    Uri,
}

impl std::fmt::Display for DnsRecordType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_string(self).unwrap_or_default();
        write!(f, "{}", s.trim_matches('"'))
    }
}

// ============================================================
// DNS Record Data (type-specific fields)
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrvData {
    pub name: String,
    pub port: u16,
    pub priority: u16,
    pub proto: String,
    pub service: String,
    pub target: String,
    pub weight: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MxData {
    pub content: String,
    pub priority: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaaData {
    pub flags: u8,
    pub tag: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshfpData {
    pub algorithm: u8,
    pub fingerprint: String,
    #[serde(rename = "type")]
    pub hash_type: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsaData {
    pub certificate: String,
    pub matching_type: u8,
    pub selector: u8,
    pub usage: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnskeyData {
    pub algorithm: u8,
    pub flags: u16,
    pub protocol: u8,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DsData {
    pub algorithm: u8,
    pub digest: String,
    pub digest_type: u8,
    pub key_tag: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UriData {
    pub content: String,
    pub priority: u16,
    pub weight: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocData {
    pub altitude: f64,
    pub lat_degrees: u8,
    pub lat_direction: String,
    pub lat_minutes: u8,
    pub lat_seconds: f64,
    pub long_degrees: u8,
    pub long_direction: String,
    pub long_minutes: u8,
    pub long_seconds: f64,
    pub precision_horz: f64,
    pub precision_vert: f64,
    pub size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NaptrData {
    pub flags: String,
    pub order: u16,
    pub preference: u16,
    pub regex: String,
    pub replacement: String,
    pub service: String,
}

// ============================================================
// DNS Record (full record returned from API)
// ============================================================

#[derive(Debug, Clone, Deserialize)]
pub struct DnsRecord {
    pub id: String,
    pub zone_id: String,
    pub zone_name: String,
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: DnsRecordType,
    pub content: Option<String>,
    pub proxiable: bool,
    pub proxied: bool,
    pub ttl: u32,
    pub locked: bool,
    pub meta: Option<serde_json::Value>,
    pub created_on: String,
    pub modified_on: String,
    pub comment: Option<String>,
    pub tags: Option<Vec<String>>,
    pub priority: Option<u16>,
    pub data: Option<serde_json::Value>,
}

// ============================================================
// Request Bodies
// ============================================================

/// Used for creating a new DNS record (POST)
#[derive(Debug, Clone, Serialize)]
pub struct CreateDnsRecord {
    #[serde(rename = "type")]
    pub record_type: DnsRecordType,
    pub name: String,
    /// Required for most record types; not used for SRV/LOC/etc that use `data`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub ttl: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// For complex record types (SRV, CAA, SSHFP, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Used for full replacement of a DNS record (PUT)
#[derive(Debug, Clone, Serialize)]
pub struct UpdateDnsRecord {
    #[serde(rename = "type")]
    pub record_type: DnsRecordType,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub ttl: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Used for partial update of a DNS record (PATCH)
#[derive(Debug, Clone, Serialize, Default)]
pub struct PatchDnsRecord {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub record_type: Option<DnsRecordType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ============================================================
// List / Export params
// ============================================================

/// Query parameters for listing DNS records
#[derive(Debug, Clone, Default, Serialize)]
pub struct ListDnsRecordsParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub record_type: Option<DnsRecordType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

/// Response from batch create/update/delete/patch operations
#[derive(Debug, Deserialize)]
pub struct BatchDnsResult {
    pub deletes: Option<Vec<serde_json::Value>>,
    pub patches: Option<Vec<DnsRecord>>,
    pub posts: Option<Vec<DnsRecord>>,
    pub puts: Option<Vec<DnsRecord>>,
}

/// Batch operation request body
#[derive(Debug, Clone, Serialize, Default)]
pub struct BatchDnsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletes: Option<Vec<BatchDeleteRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patches: Option<Vec<BatchPatchRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posts: Option<Vec<CreateDnsRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub puts: Option<Vec<BatchPutRecord>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchDeleteRecord {
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchPatchRecord {
    pub id: String,
    #[serde(flatten)]
    pub patch: PatchDnsRecord,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchPutRecord {
    pub id: String,
    #[serde(flatten)]
    pub record: UpdateDnsRecord,
}
