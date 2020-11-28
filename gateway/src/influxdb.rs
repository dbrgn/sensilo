//! Send stats to InfluxDB with async-h1.

use anyhow::{bail, Result};
use http_types::{auth::BasicAuth, Method, Request, StatusCode};
use smol::prelude::*;
use url::Url;

use crate::config;
use crate::http;
use crate::measurement::Measurement;

pub async fn submit_measurement(config: &config::InfluxDb, mmt: &Measurement<'_>) -> Result<()> {
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

    // Create request
    let url = format!("{}/write?db={}", config.connection_string, config.db);
    let mut req = Request::new(Method::Post, Url::parse(&url)?);
    req.set_body(payload);
    let auth = BasicAuth::new(&config.user, &config.pass);
    auth.apply(&mut req);

    // Send request to server
    let mut resp = http::fetch(req).await?;
    match resp.status() {
        StatusCode::NoContent => {}
        StatusCode::NotFound => {
            log::warn!("InfluxDB database {} not found", config.db);
            bail!("InfluxDB database {} not found", config.db);
        }
        StatusCode::BadRequest => {
            let mut buf = Vec::new();
            resp.read_to_end(&mut buf).await?;
            let body = String::from_utf8_lossy(&buf);
            log::debug!(
                "Could not send data to InfluxDB: Bad request: {}",
                body.trim()
            );
            bail!("Could not send data to InfluxDB: {}", resp.status())
        }
        _ => {
            bail!("Invalid status code: {}", resp.status());
        }
    }
    Ok(())
}
