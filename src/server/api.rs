use std::time::Duration;

use anyhow::{Context, Result};
use argon2::{Argon2, PasswordVerifier};
use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, method::Method},
    response::Redirect,
};
use bytes::Bytes;
use object_store::{ObjectStore, path::Path as obj_path, signer::Signer};
use serde::{Deserialize, Serialize};

use crate::server::{ServerState, db, gc};

#[derive(Serialize, Deserialize)]
pub struct StartGcReq {
    cutoff: i64,
    admin_key: String,
}

pub async fn get_root() -> &'static str {
    "nixcp <3"
}

pub async fn get_nix_cache_info(State(state): ServerState) -> String {
    let mut res = String::from("StoreDir: /nix/store\nWantMassQuery: 1");
    res.push_str(&format!("\nPriority: {}", state.priority));
    res
}

pub async fn get_narinfo(
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

pub async fn get_nar(
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

pub async fn start_gc(
    State(state): ServerState,
    Json(start_gc_req): Json<StartGcReq>,
) -> Result<(), StatusCode> {
    Argon2::default()
        .verify_password(
            start_gc_req.admin_key.as_bytes(),
            &state.admin_key_hash.password_hash(),
        )
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    tokio::task::spawn(gc::run(state, start_gc_req.cutoff));
    Ok(())
}
