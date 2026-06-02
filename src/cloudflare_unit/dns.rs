use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use crate::cloudflare_unit::entity::*;

// ============================================================
// Main Client Struct
// ============================================================

/// Cloudflare DNS API client.
///
/// # Example
/// ```no_run
/// # use anyhow::Result;
/// # async fn example() -> Result<()> {
/// use cloudflare_dns::{CloudflareDns, CreateDnsRecord, DnsRecordType};
///
/// let client = CloudflareDns::new("your_api_token", "your_zone_id")?;
///
/// let record = client.create_record(CreateDnsRecord {
///     record_type: DnsRecordType::A,
///     name: "example.com".to_string(),
///     content: Some("192.0.2.1".to_string()),
///     ttl: 3600,
///     proxied: Some(false),
///     priority: None,
///     comment: Some("My A record".to_string()),
///     tags: None,
///     data: None,
/// }).await?;
///
/// println!("Created record: {}", record.id);
/// # Ok(())
/// # }
/// ```
pub struct CloudflareDns {
    client: Client,
    api_token: String,
    zone_id: String,
    base_url: String,
}

impl CloudflareDns {
    const DEFAULT_BASE_URL: &'static str = "https://api.cloudflare.com/client/v4";

    /// Create a new `CloudflareDns` client using an API token.
    pub fn new(api_token: impl Into<String>, zone_id: impl Into<String>) -> Result<Self> {
        let api_token = api_token.into();
        let zone_id = zone_id.into();

        if api_token.is_empty() {
            return Err(CloudflareDnsError::InvalidConfig(
                "api_token must not be empty".to_string(),
            )
            .into());
        }
        if zone_id.is_empty() {
            return Err(
                CloudflareDnsError::InvalidConfig("zone_id must not be empty".to_string()).into(),
            );
        }

        let client = Client::builder()
            .user_agent("cloudflare-dns-rs/1.0")
            .build()
            .context("Failed to build reqwest client")?;

        Ok(Self {
            client,
            api_token,
            zone_id,
            base_url: Self::DEFAULT_BASE_URL.to_string(),
        })
    }

    /// Create a new `CloudflareDns` client without a zone ID.
    pub fn new_without_zone(api_token: impl Into<String>) -> Result<Self> {
        let api_token = api_token.into();

        if api_token.is_empty() {
            return Err(CloudflareDnsError::InvalidConfig(
                "api_token must not be empty".to_string(),
            )
            .into());
        }

        let client = Client::builder()
            .user_agent("cloudflare-dns-rs/1.0")
            .build()
            .context("Failed to build reqwest client")?;

        Ok(Self {
            client,
            api_token,
            zone_id: String::new(),
            base_url: Self::DEFAULT_BASE_URL.to_string(),
        })
    }

    /// Override the base URL (useful for testing / mocking).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Get Zone ID by name.
    /// `GET /zones`
    pub async fn get_zone_id_by_name(&self, name: &str) -> Result<String> {
        let url = format!("{}/zones", self.base_url);
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.api_token)
            .query(&[("name", name)])
            .send()
            .await
            .context("Failed to send get zone request")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read get zone response body")?;

        let parsed: CloudflareListResponse<ZoneResult> = serde_json::from_str(&body)
            .with_context(|| format!("Failed to parse zone list response. Raw body: {body}"))?;

        if !parsed.success {
            let message = parsed
                .errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(CloudflareDnsError::ApiError { status, message }.into());
        }

        let zones = parsed.result.unwrap_or_default();
        let zone = zones
            .into_iter()
            .find(|z| z.name == name)
            .ok_or_else(|| anyhow::anyhow!("Zone '{}' not found in Cloudflare response", name))?;

        Ok(zone.id)
    }

    // ----------------------------------------------------------
    // Helpers
    // ----------------------------------------------------------

    fn records_url(&self) -> String {
        format!("{}/zones/{}/dns_records", self.base_url, self.zone_id)
    }

    fn record_url(&self, record_id: &str) -> String {
        format!("{}/{}", self.records_url(), record_id)
    }

    fn export_url(&self) -> String {
        format!("{}/export", self.records_url())
    }

    fn import_url(&self) -> String {
        format!("{}/import", self.records_url())
    }

    fn batch_url(&self) -> String {
        format!("{}/batch", self.records_url())
    }

    fn scan_url(&self) -> String {
        format!("{}/scan", self.records_url())
    }

    async fn handle_response<T: for<'de> Deserialize<'de>>(
        response: reqwest::Response,
    ) -> Result<T> {
        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read response body")?;

        if status == StatusCode::NOT_FOUND {
            return Err(CloudflareDnsError::NotFound(body).into());
        }

        let parsed: CloudflareResponse<T> = serde_json::from_str(&body)
            .with_context(|| format!("Failed to parse Cloudflare response. Raw body: {body}"))?;

        if !parsed.success {
            let message = parsed
                .errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(CloudflareDnsError::ApiError { status, message }.into());
        }

        parsed
            .result
            .ok_or_else(|| anyhow::anyhow!("Cloudflare returned success but no result"))
    }

    async fn handle_list_response<T: for<'de> Deserialize<'de>>(
        response: reqwest::Response,
    ) -> Result<(Vec<T>, Option<ResultInfo>)> {
        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read response body")?;

        if status == StatusCode::NOT_FOUND {
            return Err(CloudflareDnsError::NotFound(body).into());
        }

        let parsed: CloudflareListResponse<T> = serde_json::from_str(&body).with_context(|| {
            format!("Failed to parse Cloudflare list response. Raw body: {body}")
        })?;

        if !parsed.success {
            let message = parsed
                .errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(CloudflareDnsError::ApiError { status, message }.into());
        }

        Ok((parsed.result.unwrap_or_default(), parsed.result_info))
    }

    // ----------------------------------------------------------
    // Public API Methods
    // ----------------------------------------------------------

    /// **List DNS Records**
    ///
    /// `GET /zones/{zone_id}/dns_records`
    ///
    /// Returns all DNS records for the zone, optionally filtered.
    pub async fn list_records(
        &self,
        params: Option<ListDnsRecordsParams>,
    ) -> Result<Vec<DnsRecord>> {
        let mut request = self
            .client
            .get(self.records_url())
            .bearer_auth(&self.api_token);

        if let Some(p) = params {
            request = request.query(&p);
        }

        let response = request
            .send()
            .await
            .context("Failed to send list request")?;
        let (records, _) = Self::handle_list_response::<DnsRecord>(response).await?;
        Ok(records)
    }

    /// **List DNS Records with pagination info**
    ///
    /// `GET /zones/{zone_id}/dns_records`
    ///
    /// Same as `list_records` but also returns `ResultInfo`.
    pub async fn list_records_with_info(
        &self,
        params: Option<ListDnsRecordsParams>,
    ) -> Result<(Vec<DnsRecord>, Option<ResultInfo>)> {
        let mut request = self
            .client
            .get(self.records_url())
            .bearer_auth(&self.api_token);

        if let Some(p) = params {
            request = request.query(&p);
        }

        let response = request
            .send()
            .await
            .context("Failed to send list request")?;
        Self::handle_list_response::<DnsRecord>(response).await
    }

    /// **Create a DNS Record**
    ///
    /// `POST /zones/{zone_id}/dns_records`
    pub async fn create_record(&self, record: CreateDnsRecord) -> Result<DnsRecord> {
        let response = self
            .client
            .post(self.records_url())
            .bearer_auth(&self.api_token)
            .json(&record)
            .send()
            .await
            .context("Failed to send create request")?;

        Self::handle_response::<DnsRecord>(response).await
    }

    /// **Get a DNS Record**
    ///
    /// `GET /zones/{zone_id}/dns_records/{dns_record_id}`
    pub async fn get_record(&self, record_id: &str) -> Result<DnsRecord> {
        let response = self
            .client
            .get(self.record_url(record_id))
            .bearer_auth(&self.api_token)
            .send()
            .await
            .context("Failed to send get request")?;

        Self::handle_response::<DnsRecord>(response).await
    }

    /// **Update (replace) a DNS Record**
    ///
    /// `PUT /zones/{zone_id}/dns_records/{dns_record_id}`
    ///
    /// This replaces the entire record. All required fields must be provided.
    pub async fn update_record(
        &self,
        record_id: &str,
        record: UpdateDnsRecord,
    ) -> Result<DnsRecord> {
        let response = self
            .client
            .put(self.record_url(record_id))
            .bearer_auth(&self.api_token)
            .json(&record)
            .send()
            .await
            .context("Failed to send update request")?;

        Self::handle_response::<DnsRecord>(response).await
    }

    /// **Patch a DNS Record**
    ///
    /// `PATCH /zones/{zone_id}/dns_records/{dns_record_id}`
    ///
    /// Partial update — only the provided fields are changed.
    pub async fn patch_record(&self, record_id: &str, patch: PatchDnsRecord) -> Result<DnsRecord> {
        let response = self
            .client
            .patch(self.record_url(record_id))
            .bearer_auth(&self.api_token)
            .json(&patch)
            .send()
            .await
            .context("Failed to send patch request")?;

        Self::handle_response::<DnsRecord>(response).await
    }

    /// **Delete a DNS Record**
    ///
    /// `DELETE /zones/{zone_id}/dns_records/{dns_record_id}`
    ///
    /// Returns the deleted record's ID on success.
    pub async fn delete_record(&self, record_id: &str) -> Result<String> {
        let response = self
            .client
            .delete(self.record_url(record_id))
            .bearer_auth(&self.api_token)
            .send()
            .await
            .context("Failed to send delete request")?;

        let result = Self::handle_response::<DeleteResult>(response).await?;
        Ok(result.id)
    }

    /// **Export DNS Records (BIND zone file)**
    ///
    /// `GET /zones/{zone_id}/dns_records/export`
    ///
    /// Returns a BIND-format zone file as a plain string.
    pub async fn export_records(&self) -> Result<String> {
        let response = self
            .client
            .get(self.export_url())
            .bearer_auth(&self.api_token)
            .send()
            .await
            .context("Failed to send export request")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(CloudflareDnsError::ApiError {
                status,
                message: body,
            }
            .into());
        }

        response.text().await.context("Failed to read export body")
    }

    /// **Import DNS Records (BIND zone file)**
    ///
    /// `POST /zones/{zone_id}/dns_records/import`
    ///
    /// Uploads a BIND zone file. Returns import result metadata as JSON value.
    pub async fn import_records(
        &self,
        zone_file_content: &str,
        proxied: Option<bool>,
    ) -> Result<serde_json::Value> {
        use reqwest::multipart;

        let mut form = multipart::Form::new().text("file", zone_file_content.to_string());
        if let Some(p) = proxied {
            form = form.text("proxied", p.to_string());
        }

        let response = self
            .client
            .post(self.import_url())
            .bearer_auth(&self.api_token)
            .multipart(form)
            .send()
            .await
            .context("Failed to send import request")?;

        Self::handle_response::<serde_json::Value>(response).await
    }

    /// **Batch DNS Operations**
    ///
    /// `POST /zones/{zone_id}/dns_records/batch`
    ///
    /// Perform multiple create / update / patch / delete operations atomically.
    pub async fn batch(&self, request: BatchDnsRequest) -> Result<BatchDnsResult> {
        let response = self
            .client
            .post(self.batch_url())
            .bearer_auth(&self.api_token)
            .json(&request)
            .send()
            .await
            .context("Failed to send batch request")?;

        Self::handle_response::<BatchDnsResult>(response).await
    }

    /// **Scan DNS Records**
    ///
    /// `POST /zones/{zone_id}/dns_records/scan`
    ///
    /// Trigger a DNS scan for the zone. Returns scan result metadata.
    pub async fn scan_records(&self) -> Result<serde_json::Value> {
        let response = self
            .client
            .post(self.scan_url())
            .bearer_auth(&self.api_token)
            .send()
            .await
            .context("Failed to send scan request")?;

        Self::handle_response::<serde_json::Value>(response).await
    }

    // ----------------------------------------------------------
    // Convenience helpers
    // ----------------------------------------------------------

    /// Create a simple **A** record.
    pub async fn create_a_record(
        &self,
        name: impl Into<String>,
        ipv4: impl Into<String>,
        ttl: u32,
        proxied: bool,
    ) -> Result<DnsRecord> {
        self.create_record(CreateDnsRecord {
            record_type: DnsRecordType::A,
            name: name.into(),
            content: Some(ipv4.into()),
            ttl,
            proxied: Some(proxied),
            priority: None,
            comment: None,
            tags: None,
            data: None,
        })
        .await
    }

    /// Create a simple **AAAA** record.
    pub async fn create_aaaa_record(
        &self,
        name: impl Into<String>,
        ipv6: impl Into<String>,
        ttl: u32,
        proxied: bool,
    ) -> Result<DnsRecord> {
        self.create_record(CreateDnsRecord {
            record_type: DnsRecordType::Aaaa,
            name: name.into(),
            content: Some(ipv6.into()),
            ttl,
            proxied: Some(proxied),
            priority: None,
            comment: None,
            tags: None,
            data: None,
        })
        .await
    }

    /// Create a simple **CNAME** record.
    pub async fn create_cname_record(
        &self,
        name: impl Into<String>,
        target: impl Into<String>,
        ttl: u32,
        proxied: bool,
    ) -> Result<DnsRecord> {
        self.create_record(CreateDnsRecord {
            record_type: DnsRecordType::Cname,
            name: name.into(),
            content: Some(target.into()),
            ttl,
            proxied: Some(proxied),
            priority: None,
            comment: None,
            tags: None,
            data: None,
        })
        .await
    }

    /// Create a **TXT** record.
    pub async fn create_txt_record(
        &self,
        name: impl Into<String>,
        content: impl Into<String>,
        ttl: u32,
    ) -> Result<DnsRecord> {
        self.create_record(CreateDnsRecord {
            record_type: DnsRecordType::Txt,
            name: name.into(),
            content: Some(content.into()),
            ttl,
            proxied: Some(false),
            priority: None,
            comment: None,
            tags: None,
            data: None,
        })
        .await
    }

    /// Create an **MX** record.
    pub async fn create_mx_record(
        &self,
        name: impl Into<String>,
        mail_server: impl Into<String>,
        priority: u16,
        ttl: u32,
    ) -> Result<DnsRecord> {
        self.create_record(CreateDnsRecord {
            record_type: DnsRecordType::Mx,
            name: name.into(),
            content: Some(mail_server.into()),
            ttl,
            proxied: Some(false),
            priority: Some(priority),
            comment: None,
            tags: None,
            data: None,
        })
        .await
    }

    /// Create an **NS** record.
    pub async fn create_ns_record(
        &self,
        name: impl Into<String>,
        nameserver: impl Into<String>,
        ttl: u32,
    ) -> Result<DnsRecord> {
        self.create_record(CreateDnsRecord {
            record_type: DnsRecordType::Ns,
            name: name.into(),
            content: Some(nameserver.into()),
            ttl,
            proxied: Some(false),
            priority: None,
            comment: None,
            tags: None,
            data: None,
        })
        .await
    }

    /// Create a **PTR** record.
    pub async fn create_ptr_record(
        &self,
        name: impl Into<String>,
        target: impl Into<String>,
        ttl: u32,
    ) -> Result<DnsRecord> {
        self.create_record(CreateDnsRecord {
            record_type: DnsRecordType::Ptr,
            name: name.into(),
            content: Some(target.into()),
            ttl,
            proxied: Some(false),
            priority: None,
            comment: None,
            tags: None,
            data: None,
        })
        .await
    }

    /// Create an **SRV** record using structured data.
    pub async fn create_srv_record(
        &self,
        name: impl Into<String>,
        srv: SrvData,
        ttl: u32,
    ) -> Result<DnsRecord> {
        let data = serde_json::to_value(srv).context("Failed to serialize SRV data")?;
        self.create_record(CreateDnsRecord {
            record_type: DnsRecordType::Srv,
            name: name.into(),
            content: None,
            ttl,
            proxied: Some(false),
            priority: None,
            comment: None,
            tags: None,
            data: Some(data),
        })
        .await
    }

    /// Create a **CAA** record.
    pub async fn create_caa_record(
        &self,
        name: impl Into<String>,
        caa: CaaData,
        ttl: u32,
    ) -> Result<DnsRecord> {
        let data = serde_json::to_value(caa).context("Failed to serialize CAA data")?;
        self.create_record(CreateDnsRecord {
            record_type: DnsRecordType::Caa,
            name: name.into(),
            content: None,
            ttl,
            proxied: Some(false),
            priority: None,
            comment: None,
            tags: None,
            data: Some(data),
        })
        .await
    }

    /// Create an **SSHFP** record.
    pub async fn create_sshfp_record(
        &self,
        name: impl Into<String>,
        sshfp: SshfpData,
        ttl: u32,
    ) -> Result<DnsRecord> {
        let data = serde_json::to_value(sshfp).context("Failed to serialize SSHFP data")?;
        self.create_record(CreateDnsRecord {
            record_type: DnsRecordType::Sshfp,
            name: name.into(),
            content: None,
            ttl,
            proxied: Some(false),
            priority: None,
            comment: None,
            tags: None,
            data: Some(data),
        })
        .await
    }

    /// Create a **TLSA** record.
    pub async fn create_tlsa_record(
        &self,
        name: impl Into<String>,
        tlsa: TlsaData,
        ttl: u32,
    ) -> Result<DnsRecord> {
        let data = serde_json::to_value(tlsa).context("Failed to serialize TLSA data")?;
        self.create_record(CreateDnsRecord {
            record_type: DnsRecordType::Tlsa,
            name: name.into(),
            content: None,
            ttl,
            proxied: Some(false),
            priority: None,
            comment: None,
            tags: None,
            data: Some(data),
        })
        .await
    }

    /// Find records by name (exact match).
    pub async fn find_records_by_name(&self, name: &str) -> Result<Vec<DnsRecord>> {
        self.list_records(Some(ListDnsRecordsParams {
            name: Some(name.to_string()),
            ..Default::default()
        }))
        .await
    }

    /// Find records by type.
    pub async fn find_records_by_type(&self, record_type: DnsRecordType) -> Result<Vec<DnsRecord>> {
        self.list_records(Some(ListDnsRecordsParams {
            record_type: Some(record_type),
            ..Default::default()
        }))
        .await
    }

    /// Delete all DNS records matching a name.
    pub async fn delete_records_by_name(&self, name: &str) -> Result<Vec<String>> {
        let records = self.find_records_by_name(name).await?;
        let mut deleted_ids = Vec::new();

        for record in records {
            let id = self.delete_record(&record.id).await?;
            deleted_ids.push(id);
        }

        Ok(deleted_ids)
    }
}
