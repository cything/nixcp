use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow};
use argon2::password_hash::PasswordHashString;
use argon2::{Argon2, PasswordVerifier};
use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use object_store::aws::{AmazonS3, AmazonS3ConfigKey};
use sea_orm::{Database, DatabaseConnection};
use tokio::net::TcpListener;
use url::Url;

use crate::{ServeArgs, utils};

pub mod api;
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
    admin_key_hash: PasswordHashString,
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
            admin_key_hash: PasswordHashString::new(cli.admin_key_hash.as_str())
                .map_err(|e| anyhow!("Invalid admin key hash: {:?}", e))?,
            endpoint,
            db_conn,
        })
    }

    pub async fn start_server(self) -> Result<()> {
        db::run_migrations(&self.db_conn).await?;

        let addr = SocketAddr::new(self.host, self.port);
        let listener = TcpListener::bind(addr).await?;
        eprintln!("Listening on {addr}");

        let state = Arc::new(self);
        let app = Router::new()
            .route("/gc/start", post(api::start_gc))
            .route_layer(middleware::from_fn_with_state(state.clone(), auth))
            .route("/", get(api::get_root))
            .route("/nix-cache-info", get(api::get_nix_cache_info))
            .route("/{hash}.narinfo", get(api::get_narinfo))
            .route("/nar/{nar}", get(api::get_nar))
            .with_state(state);

        axum::serve(listener, app).await.context("axum serve")?;
        Ok(())
    }
}

async fn auth(
    State(state): ServerState,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    match headers.get("Admin-Key").and_then(|x| x.to_str().ok()) {
        Some(admin_key) if is_admin_key_valid(admin_key, &state.admin_key_hash) => {
            let response = next.run(request).await;
            Ok(response)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

fn is_admin_key_valid(admin_key: &str, admin_key_hash: &PasswordHashString) -> bool {
    Argon2::default()
        .verify_password(admin_key.as_bytes(), &admin_key_hash.password_hash())
        .is_ok()
}
