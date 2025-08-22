use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router,
    extract::{Path, State},
    routing::get,
};
use object_store::aws::AmazonS3;
use sea_orm::{Database, DatabaseConnection};
use time::UtcDateTime;
use tokio::net::TcpListener;

use crate::{ServeArgs, utils};

mod db;

#[derive(Clone)]
pub struct ServerConfig {
    s3: AmazonS3,
    host: IpAddr,
    port: u16,
    priority: i32,
    db_conn: DatabaseConnection,
}

impl ServerConfig {
    pub async fn new(cli: &ServeArgs) -> Result<Self> {
        let s3_builder =
            utils::s3_builder_from_cli(&cli.bucket, cli.region.as_deref(), cli.endpoint.as_deref());
        let db_conn = Database::connect("sqlite://./db.sqlite?mode=rwc").await?;


        Ok(Self {
            s3: s3_builder.build()?,
            host: cli.host.parse()?,
            port: cli.port,
            priority: cli.priority,
            db_conn,
        })
    }

    pub async fn start_server(self) -> Result<()> {
        db::run_migrations(&self.db_conn).await?;

        let addr = SocketAddr::new(self.host, self.port);
        let listener = TcpListener::bind(addr).await?;

        let app = Router::new()
            .route("/", get(get_root))
            .route("/nix-cache-info", get(get_nix_cache_info))
            .route("/{hash}.narinfo", get(get_narinfo))
            .with_state(self);

        Ok(axum::serve(listener, app).await?)
    }
}

async fn get_root() -> &'static str {
    "nixcp <3"
}

async fn get_nix_cache_info(State(state): State<ServerConfig>) -> String {
    let mut res = String::from("StoreDir: /nix/store\nWantMassQuery: 1");
    res.push_str(&format!("\nPriority: {}", state.priority));
    res
}

async fn get_narinfo(Path(hash): Path<String>, State(state): State<ServerConfig>) -> String {
    let last_access = db::get_narinfo_last_access(&state.db_conn, &hash).await;
    db::update_narinfo_last_access(&state.db_conn, hash.as_str())
        .await
        .unwrap();

    let mut res = format!("you requested nar hash: {hash}");
    if let Ok(last_access) = last_access {
        let last_access = UtcDateTime::from_unix_timestamp(last_access).unwrap();
        res.push_str(&format!("\nlast accessed on {last_access}"));
    }
    res
}
