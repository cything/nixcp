use crate::protocol::{Request, Response};
use anyhow::{Error, Result};
use bincode::{config::standard, decode_from_slice, encode_into_slice};
use bytes::BytesMut;
use futures::{SinkExt, StreamExt, TryStreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::spawn;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::info;

pub const DEFAULT_PORT: u16 = 42069;

async fn run_server() -> Result<()> {
    let listener = TcpListener::bind(format!("127.0.0.1:{DEFAULT_PORT}")).await?;
    loop {
        let (socket, _) = listener.accept().await?;
        spawn(handler(socket));
    }
}

fn handler(socket: TcpStream) -> impl Future<Output = Result<()>> {
    let io = Framed::new(socket, LengthDelimitedCodec::new())
        .err_into::<Error>()
        .sink_err_into::<Error>();
    let (sink, stream) = io.split();

    stream
        .and_then(|bytes| async move {
            decode_from_slice::<Request, _>(&bytes, standard()).map_err(Error::from)
        })
        .and_then(|(req, _)| async move {
            let mut res = BytesMut::new();
            match req {
                Request::Upload(upload) => {
                    info!("client sent path: {}", upload.path);
                    encode_into_slice(Response::Upload, &mut res, standard())
                        .map_err(std::io::Error::other)?;
                }
            }
            Ok(res.freeze())
        })
        .forward(sink)
}
