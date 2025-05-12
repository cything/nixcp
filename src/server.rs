use std::time::Duration;

use crate::protocol::{Request, Response};
use anyhow::{Context, Error, Result, bail};
use bincode::{config::standard, decode_from_slice, encode_to_vec};
use bytes::Bytes;
use futures::{SinkExt, StreamExt, TryStreamExt};
use std::pin::pin;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::spawn;
use tokio::time::timeout;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{debug, info};

const DEFAULT_ADDR: &str = "127.0.0.1:42069";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub async fn run_server() -> Result<()> {
    let listener = TcpListener::bind(DEFAULT_ADDR).await?;
    info!("Listening on {}", listener.local_addr()?);
    loop {
        let (socket, _) = listener.accept().await?;
        if let Ok(addr) = socket.peer_addr() {
            info!("Handling connection from {addr}");
        }
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
            match req {
                Request::Upload(upload) => {
                    debug!("client sent path: {}", upload.path);
                    encode_to_vec(Response::Upload, standard())
                        .map(Bytes::from)
                        .map_err(Error::from)
                }
                Request::Ping => {
                    debug!("ping from a client");
                    encode_to_vec(Response::Pong, standard())
                        .map(Bytes::from)
                        .map_err(Error::from)
                }
            }
        })
        .forward(sink)
}

pub async fn connect_to_server() -> Option<TcpStream> {
    let connect = TcpStream::connect(DEFAULT_ADDR);
    match timeout(CONNECT_TIMEOUT, connect).await {
        Ok(Ok(stream)) => Some(stream),
        _ => None,
    }
}

pub async fn ping_pong(stream: TcpStream) -> Result<()> {
    let io = Framed::new(stream, LengthDelimitedCodec::new())
        .err_into::<Error>()
        .sink_err_into::<Error>();
    let (mut sink, stream) = io.split();

    let req = encode_to_vec(Request::Ping, standard()).context("encode Request:Ping")?;
    sink.send(req.into()).await.context("send ping")?;

    let mut stream = pin!(stream.and_then(|bytes| async move {
        decode_from_slice::<Response, _>(&bytes, standard())
            .map_err(Error::from)
            .context("decode response")
    }));

    match timeout(REQUEST_TIMEOUT, stream.try_next()).await {
        Ok(Ok(Some((res, _)))) => match res {
            Response::Pong => Ok(()),
            _ => bail!("Response something other than pong"),
        },
        Err(e) => bail!("Request timeout expired: {e}"),
        _ => bail!("Did not receive a response"),
    }
}
