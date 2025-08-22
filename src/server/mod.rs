use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use anyhow::Result;
use axum::{Router, routing::get, extract::State};
use object_store::aws::AmazonS3;
use tokio::net::TcpListener;

use crate::{ServeArgs, utils};

pub struct ServerConfig {
    s3: Arc<AmazonS3>,
    host: IpAddr,
    port: u16,
    priority: i32,
}

impl ServerConfig {
    pub async fn new(cli: &ServeArgs) -> Result<Self> {
        let s3_builder = utils::s3_builder_from_cli(
            &cli.bucket,
            cli.region.as_deref(),
            cli.endpoint.as_deref(),
        );

        Ok(Self {
            s3: Arc::new(s3_builder.build()?),
            host: cli.host.parse()?,
            port: cli.port,
            priority: cli.priority,
        })
    }
}

pub async fn start_server(config: Arc<ServerConfig>) -> Result<()> {
    let app = Router::new()
        .route("/", get(get_root))
        .route("/nix-cache-info", get(get_nix_cache_info))
        .with_state(Arc::clone(&config));

    let addr = SocketAddr::new(config.host, config.port);
    let listener = TcpListener::bind(addr).await?;

    Ok(axum::serve(listener, app).await?)
}

async fn get_root() -> &'static str {
    "nixcp <3"
}

async fn get_nix_cache_info(State(state): State<Arc<ServerConfig>>) -> String {
    let mut res = String::from("StoreDir: /nix/store\nWantMassQuery: 1");
    res.push_str(&format!("\nPriority: {}", state.priority));
    res
}
