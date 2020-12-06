//! Send stats to InfluxDB with async-h1.
use std::time::Duration;

use anyhow::{bail, Result};
use ureq::Agent;

use crate::config;
use crate::measurement::Measurement;

/// Create an ureq agent.
pub fn make_ureq_agent() -> Agent {
    ureq::AgentBuilder::new()
        .timeout_read(Duration::from_secs(5))
        .timeout_write(Duration::from_secs(5))
        .build()
}

pub async fn submit_measurement(
    agent: Agent,
    config: &config::InfluxDb,
    mmt: &Measurement<'_>,
) -> Result<()> {
    // Prepare payloads
    let mut payloads = vec![];
    let tags = format!("address={},local_name={}", mmt.address, mmt.local_name);
    payloads.push(format!("rssi,{} value={}", tags, mmt.rssi));
    payloads.push(format!("counter,{} value={}", tags, mmt.counter));
    if let Some(ref temp) = mmt.temperature {
        payloads.push(format!(
            "temperature,{} value={}",
            tags,
            temp.as_millidegrees_celsius()
        ));
    }
    if let Some(ref humi) = mmt.humidity {
        payloads.push(format!(
            "humidity,{} value={}",
            tags,
            humi.as_millipercent()
        ));
    }
    if let Some(ref lux) = mmt.ambient_light {
        payloads.push(format!("ambient_light,{} value={:.2}", tags, lux.as_lux()));
    }
    let payload = payloads.join("\n");

    // Create basic auth header
    let auth = format!(
        "Basic {}",
        base64::encode(format!("{}:{}", &config.user, &config.pass))
    );
    println!("Auth: {:?}", auth);

    // Create request
    let url = format!("{}/write?db={}", config.connection_string, config.db);

    // Send request to server
    let resp: ureq::Response = smol::unblock(move || {
        agent
            .post(&url)
            .set("authorization", &auth)
            .error_on_non_2xx(false)
            .send_string(&payload)
    })
    .await?;

    // Handle response
    match resp.status() {
        // No content
        204 => {}
        // Not found
        404 => {
            log::warn!("InfluxDB database {} not found", config.db);
            bail!("InfluxDB database {} not found", config.db);
        }
        // Bad request, permission denied
        400 | 401 => {
            let status = format!("{} ({})", resp.status(), resp.status_text());
            let body = resp
                .into_string()
                .unwrap_or_else(|e| format!("[response decode error: {}]", e));
            log::debug!(
                "Could not send data to InfluxDB: Bad request: {}",
                body.trim()
            );
            bail!("Could not send data to InfluxDB: {}", status)
        }
        _ => {
            let status = format!("{} ({})", resp.status(), resp.status_text());
            bail!("Invalid status code: {}", status);
        }
    }
    Ok(())
}
