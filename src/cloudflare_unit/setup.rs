use crate::cloudflare_unit::entity::DnsRecordType;
use crate::cloudflare_unit::{dns::CloudflareDns, entity::CreateDnsRecord};
use crate::config::Config;
use crate::helper::config::config_get_nodes;
use anyhow::{Context, Result};

/// Update DNS records on Cloudflare for the given app or all apps.
///
/// - `app_name`: Optional target app name to update. If `None`, updates all apps in the config.
/// - `is_deploy`: If `true` (called during deploy), missing token logs a warning and returns Ok.
///   If `false` (called explicitly via CLI), missing token returns an Error.
pub async fn update_dns(config: &Config, app_name: Option<&str>, is_deploy: bool) -> Result<()> {
    let token = match std::env::var("CLOUDFLARE_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => {
            if is_deploy {
                println!("Warning: CLOUDFLARE_TOKEN not set in environment. Skipping DNS updates.");
                return Ok(());
            } else {
                anyhow::bail!("CLOUDFLARE_TOKEN environment variable is not set.");
            }
        }
    };

    // Filter apps to update
    let target_apps: Vec<_> = if let Some(target) = app_name {
        let app = config
            .app
            .iter()
            .find(|(id, a)| *id == target || a.name == target)
            .map(|(_, a)| a)
            .ok_or_else(|| anyhow::anyhow!("App '{}' not found in configuration", target))?;
        vec![app]
    } else {
        config.app.values().collect()
    };

    if target_apps.is_empty() {
        println!("No apps configured for DNS updates.");
        return Ok(());
    }

    let app_nodes = config_get_nodes(&config, "app");

    if app_nodes.is_empty() {
        println!("No nodes with role 'app' configured. Skipping DNS updates.");
        return Ok(());
    }

    for app in target_apps {
        let app_domain = app.domain.clone().unwrap_or_else(|| {
            config
                .domain
                .as_ref()
                .map(|d| d.domain_name.clone())
                .unwrap_or_else(|| "localhost".to_string())
        });

        // Skip local/dev domains
        if app_domain == "localhost"
            || app_domain.ends_with(".localhost")
            || app_domain.ends_with(".local")
        {
            println!(
                "Skipping DNS update for local domain '{}' (app: {})",
                app_domain, app.name
            );
            continue;
        }

        // get zone name
        let zone_name = get_zone_name(&app_domain, config);
        println!(
            "\nUpdating DNS for app '{}' ({}) using zone '{}'",
            app.name, app_domain, zone_name
        );

        // create cloudflare client
        let client = CloudflareDns::new_without_zone(&token)?;
        let zone_id = client
            .get_zone_id_by_name(&zone_name)
            .await
            .with_context(|| {
                format!(
                    "Failed to resolve Zone ID for domain '{}' (using zone name '{}')",
                    app_domain, zone_name
                )
            })?;

        let dns_client = CloudflareDns::new(&token, &zone_id)?;

        // Find existing records created by Crane for this app
        let comment_to_match = format!("#crane_{}", app.name);
        let records = dns_client.list_records(None).await?;
        let records_to_delete: Vec<_> = records
            .into_iter()
            .filter(|r| r.comment.as_deref() == Some(&comment_to_match))
            .collect();

        // Delete old records
        for r in records_to_delete {
            println!(
                "\tDeleting old DNS record for {} (IP: {:?})",
                app_domain,
                r.content.as_deref().unwrap_or("")
            );
            dns_client.delete_record(&r.id).await?;
        }

        // Create new records for all app node public IPs
        for node in &app_nodes {
            println!(
                "\tCreating DNS record pointing {} -> {} (comment: {})",
                app_domain, node.public_ip, comment_to_match
            );

            let record = CreateDnsRecord {
                record_type: DnsRecordType::A,
                name: app_domain.clone(),
                content: Some(node.public_ip.clone()),
                ttl: 3600,
                proxied: Some(true),
                priority: None,
                comment: Some(comment_to_match.clone()),
                tags: None,
                data: None,
            };

            dns_client.create_record(record).await?;
        }
    }

    Ok(())
}

/// Synchronous wrapper around `update_dns` using a Tokio runtime.
pub fn update_dns_blocking(config: &Config, app_name: Option<&str>, is_deploy: bool) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to build tokio runtime for DNS update")?;
    rt.block_on(update_dns(config, app_name, is_deploy))
}

/// Helper to get the Cloudflare zone name (apex domain) from an app domain.
fn get_zone_name(app_domain: &str, config: &Config) -> String {
    if let Some(ref dom) = config.domain {
        if app_domain == dom.domain_name {
            return dom.domain_name.clone();
        }
        if app_domain.ends_with(&format!(".{}", dom.domain_name)) {
            return dom.domain_name.clone();
        }
    }

    // Fallback: extract last two segments
    let parts: Vec<&str> = app_domain.split('.').collect();
    if parts.len() >= 2 {
        format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        app_domain.to_string()
    }
}
