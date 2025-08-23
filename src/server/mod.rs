use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
    sync::Arc,
};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Path, State},
    http::{StatusCode, method::Method},
    response::Redirect,
    routing::get,
};
use bytes::Bytes;
use object_store::{
    ObjectStore,
    aws::{AmazonS3, AmazonS3ConfigKey},
    path::Path as obj_path,
    signer::Signer,
};
use sea_orm::{Database, DatabaseConnection};
use tokio::net::TcpListener;
use url::Url;

use crate::{ServeArgs, utils};

mod db;
mod gc;

pub struct ServerConfig {
    s3: AmazonS3,
    host: IpAddr,
    port: u16,
    priority: i32,
    db_conn: DatabaseConnection,
    endpoint: Url,
    s3_sign_url: bool,
}

type ServerState = State<Arc<ServerConfig>>;

impl ServerConfig {
    pub async fn new(cli: &ServeArgs) -> Result<Self> {
        let s3_builder =
            utils::s3_builder_from_cli(&cli.bucket, cli.region.as_deref(), cli.endpoint.as_deref());
        let db_conn = Database::connect("sqlite://./db.sqlite?mode=rwc").await?;
        let endpoint = Url::parse(
            s3_builder
                .get_config_value(&AmazonS3ConfigKey::Endpoint)
                .context("get endpoint config from s3_builder")?
                .as_str(),
        )?;

        Ok(Self {
            s3: s3_builder.build()?,
            host: cli.host.parse()?,
            port: cli.port,
            priority: cli.priority,
            s3_sign_url: !cli.s3_no_sign_url,
            endpoint,
            db_conn,
        })
    }

    pub async fn start_server(self) -> Result<()> {
        db::run_migrations(&self.db_conn).await?;

        let addr = SocketAddr::new(self.host, self.port);
        let listener = TcpListener::bind(addr).await?;
        eprintln!("Listening on {addr}");

        let app = Router::new()
            .route("/", get(get_root))
            .route("/nix-cache-info", get(get_nix_cache_info))
            .route("/{hash}.narinfo", get(get_narinfo))
            .route("/nar/{nar}", get(get_nar))
            .with_state(Arc::new(self));

        axum::serve(listener, app).await.context("axum serve")?;
        Ok(())
    }
}

async fn get_root() -> &'static str {
    "nixcp <3"
}

async fn get_nix_cache_info(State(state): ServerState) -> String {
    let mut res = String::from("StoreDir: /nix/store\nWantMassQuery: 1");
    res.push_str(&format!("\nPriority: {}", state.priority));
    res
}

async fn get_narinfo(
    Path(hash): Path<String>,
    State(state): ServerState,
) -> Result<Bytes, StatusCode> {
    let narinfo_data = state
        .s3
        .get(&obj_path::from(format!("{hash}.narinfo").as_str()))
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    db::update_narinfo_last_access(&state.db_conn, hash.as_str())
        .await
        .unwrap();

    narinfo_data
        .bytes()
        .await
        .map_err(|_| StatusCode::NOT_FOUND)
}

async fn get_nar(
    Path(nar): Path<String>,
    State(state): ServerState,
) -> Result<Redirect, StatusCode> {
    let redir_target = if state.s3_sign_url {
        state
            .s3
            .signed_url(
                Method::GET,
                &obj_path::from(format!("nar/{nar}").as_str()),
                Duration::from_secs(60 * 60),
            )
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?
            .to_string()
    } else {
        state
            .endpoint
            .join(&format!("nar/{nar}"))
            .context("join nar path into endpoint")
            .unwrap()
            .to_string()
    };

    Ok(Redirect::temporary(&redir_target))
}
