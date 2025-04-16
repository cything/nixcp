use std::{ffi::OsStr, os::unix::ffi::OsStrExt, sync::Arc};

use anyhow::{Context, Result};
use nix_compat::store_path::StorePath;
use tokio::task;

use crate::{bindings, path_info::PathInfo};

pub struct Store {
    inner: Arc<bindings::FfiNixStore>,
}

impl Store {
    pub fn connect() -> Result<Self> {
        let inner = unsafe { bindings::open_nix_store()? };
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub async fn compute_fs_closure(
        &self,
        path: StorePath<String>,
    ) -> Result<Vec<StorePath<String>>> {
        let inner = self.inner.clone();
        task::spawn_blocking(move || {
            let cxx_vector =
                inner
                    .store()
                    .compute_fs_closure(path.to_string().as_bytes(), false, true, true)?;
            Ok(cxx_vector
                .iter()
                .map(|x| {
                    StorePath::from_bytes(x.as_bytes())
                        .context("make StorePath from vector returned by compute_fs_closure")
                })
                .collect::<Result<_, _>>()?)
        })
        .await
        .unwrap()
    }

    pub async fn query_path_info(&self, path: StorePath<String>) -> Result<PathInfo> {
        let inner = self.inner.clone();

        task::spawn_blocking(move || {
            let mut c_path_info = inner.store().query_path_info(path.to_string().as_bytes())?;

            let signatures = c_path_info
                .pin_mut()
                .sigs()
                .into_iter()
                .map(|x| {
                    let osstr = OsStr::from_bytes(x.as_bytes());
                    osstr.to_str().unwrap().to_string()
                })
                .collect();
            let references = c_path_info
                .pin_mut()
                .references()
                .into_iter()
                .map(|x| StorePath::from_bytes(x.as_bytes()))
                .collect::<Result<_, _>>()?;

            Ok(PathInfo {
                path,
                signatures,
                references,
            })
        })
        .await
        .unwrap()
    }
}
