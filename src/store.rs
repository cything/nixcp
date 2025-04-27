use std::{ffi::OsStr, os::unix::ffi::OsStrExt, sync::Arc};

use anyhow::{Context, Result};
use nix_compat::store_path::StorePath;
use tokio::{io::AsyncRead, task};
use tokio_util::io::StreamReader;

use crate::{
    bindings::{self, AsyncWriteAdapter},
    path_info::PathInfo,
};

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
            cxx_vector
                .iter()
                .map(|x| {
                    StorePath::from_bytes(x.as_bytes())
                        .context("make StorePath from vector returned by compute_fs_closure")
                })
                .collect::<Result<_, _>>()
        })
        .await
        .unwrap()
    }

    pub async fn query_path_info(&self, path: StorePath<String>) -> Result<PathInfo> {
        let inner = self.inner.clone();

        task::spawn_blocking(move || {
            let mut c_path_info = inner
                .store()
                .query_path_info(path.to_string().as_bytes())
                .context("query cpp for path info")?;

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
                .collect::<Result<_, _>>()
                .context("get references from pathinfo")?;
            let nar_size = c_path_info.pin_mut().nar_size();

            Ok(PathInfo {
                path,
                signatures,
                references,
                nar_size,
            })
        })
        .await
        .unwrap()
    }

    pub fn nar_from_path(&self, store_path: StorePath<String>) -> impl AsyncRead {
        let inner = self.inner.clone();
        let (adapter, mut sender) = AsyncWriteAdapter::new();
        let base_name = store_path.to_string().as_bytes().to_vec();

        tokio::task::spawn_blocking(move || {
            // Send all exceptions through the channel, and ignore errors
            // during sending (the channel may have been closed).
            if let Err(e) = inner.store().nar_from_path(base_name, sender.clone()) {
                let _ = sender.rust_error(e);
            }
        });

        StreamReader::new(adapter)
    }
}
