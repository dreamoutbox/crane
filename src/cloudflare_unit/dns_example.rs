// ============================================================
// Runnable example (requires CLOUDFLARE_API_TOKEN + ZONE_ID env vars)
// ============================================================

async fn dns_example() -> Result<()> {
    let token =
        std::env::var("CLOUDFLARE_API_TOKEN").context("Missing CLOUDFLARE_API_TOKEN env var")?;
    let zone_id =
        std::env::var("CLOUDFLARE_ZONE_ID").context("Missing CLOUDFLARE_ZONE_ID env var")?;

    let dns = CloudflareDns::new(token, zone_id)?;

    // --- List all records ---
    println!("=== All DNS Records ===");
    let records = dns.list_records(None).await?;
    for r in &records {
        println!("  [{:}] {} -> {:?}", r.record_type, r.name, r.content);
    }

    // --- Create an A record ---
    println!("\n=== Creating A record ===");
    let new_record = dns
        .create_a_record("test.example.com", "198.51.100.42", 3600, false)
        .await?;
    println!("  Created: {} (id={})", new_record.name, new_record.id);

    // --- Get the record ---
    println!("\n=== Getting record ===");
    let fetched = dns.get_record(&new_record.id).await?;
    println!("  Fetched: {} ttl={}", fetched.name, fetched.ttl);

    // --- Patch (partial update) the record ---
    println!("\n=== Patching record (change TTL) ===");
    let patched = dns
        .patch_record(
            &new_record.id,
            PatchDnsRecord {
                ttl: Some(7200),
                comment: Some("Updated via API".to_string()),
                ..Default::default()
            },
        )
        .await?;
    println!("  Patched: {} ttl={}", patched.name, patched.ttl);

    // --- Full update (PUT) ---
    println!("\n=== Full update (PUT) ===");
    let updated = dns
        .update_record(
            &new_record.id,
            UpdateDnsRecord {
                record_type: DnsRecordType::A,
                name: "test.example.com".to_string(),
                content: Some("203.0.113.10".to_string()),
                ttl: 1800,
                proxied: Some(false),
                priority: None,
                comment: Some("Full replacement".to_string()),
                tags: None,
                data: None,
            },
        )
        .await?;
    println!("  Updated content: {:?}", updated.content);

    // --- Create an MX record ---
    println!("\n=== Creating MX record ===");
    let mx = dns
        .create_mx_record("example.com", "mail.example.com", 10, 3600)
        .await?;
    println!("  MX created: {} priority={:?}", mx.name, mx.priority);

    // --- Create a TXT record ---
    println!("\n=== Creating TXT record ===");
    let txt = dns
        .create_txt_record("example.com", "v=spf1 include:_spf.example.com ~all", 3600)
        .await?;
    println!("  TXT created: {}", txt.id);

    // --- Create an SRV record ---
    println!("\n=== Creating SRV record ===");
    let srv = dns
        .create_srv_record(
            "_sip._tcp.example.com",
            SrvData {
                name: "example.com".to_string(),
                port: 5060,
                priority: 10,
                proto: "_tcp".to_string(),
                service: "_sip".to_string(),
                target: "sip.example.com".to_string(),
                weight: 20,
            },
            3600,
        )
        .await?;
    println!("  SRV created: {}", srv.id);

    // --- Batch operations ---
    println!("\n=== Batch operation ===");
    let batch_result = dns
        .batch(BatchDnsRequest {
            posts: Some(vec![CreateDnsRecord {
                record_type: DnsRecordType::A,
                name: "batch1.example.com".to_string(),
                content: Some("10.0.0.1".to_string()),
                ttl: 3600,
                proxied: Some(false),
                priority: None,
                comment: None,
                tags: None,
                data: None,
            }]),
            deletes: Some(vec![BatchDeleteRecord { id: mx.id.clone() }]),
            ..Default::default()
        })
        .await?;
    println!(
        "  Batch done — created: {}, deleted: {}",
        batch_result.posts.as_ref().map(|v| v.len()).unwrap_or(0),
        batch_result.deletes.as_ref().map(|v| v.len()).unwrap_or(0)
    );

    // --- Export zone file ---
    println!("\n=== Export zone file ===");
    let zone_file = dns.export_records().await?;
    println!(
        "  First 200 chars:\n{}",
        &zone_file[..zone_file.len().min(200)]
    );

    // --- Scan ---
    println!("\n=== Scan ===");
    let scan = dns.scan_records().await?;
    println!("  Scan result: {scan}");

    // --- Find by name ---
    println!("\n=== Find by name ===");
    let found = dns.find_records_by_name("test.example.com").await?;
    println!("  Found {} record(s) for test.example.com", found.len());

    // --- Delete our test records ---
    println!("\n=== Deleting records ===");
    let deleted = dns.delete_record(&new_record.id).await?;
    println!("  Deleted record id={deleted}");

    let deleted_ids = dns.delete_records_by_name("batch1.example.com").await?;
    println!("  Deleted by name: {deleted_ids:?}");

    Ok(())
}
