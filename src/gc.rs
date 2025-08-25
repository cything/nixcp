use anyhow::Result;
use time::UtcDateTime;
use url::Url;

use crate::GcArgs;
use crate::server::api;

pub async fn start(cli: &GcArgs) -> Result<()> {
    let gc_duration: std::time::Duration = cli.duration.parse::<humantime::Duration>()?.into();
    let cutoff = UtcDateTime::now() - gc_duration;
    let req = api::StartGcReq {
        admin_key: cli.admin_key.clone(),
        cutoff: cutoff.unix_timestamp(),
    };

    let server = Url::parse(&cli.server).map_err(|e| {
        eprint!("Invalid server URL");
        e
    })?;
    let client = reqwest::Client::new();
    let res = client
        .post(server.join("/gc/start")?)
        .json(&req)
        .send()
        .await?;
    if res.status().is_success() {
        println!("Garbage collection started successfully.");
    }

    Ok(())
}
