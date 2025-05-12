use bincode::{Decode, Encode};

/// Client request
#[derive(Encode, Decode)]
pub enum Request {
    /// Upload a path
    Upload(Upload),
    /// Check if a server is active
    Ping,
}

/// Server request
#[derive(Encode, Decode)]
pub enum Response {
    /// Response for `Request::Upload`
    Upload,
    /// Response for `Request::Ping`
    Pong,
}

/// Contents of compile request
#[derive(Encode, Decode)]
pub struct Upload {
    /// The store path to upload
    pub path: String,
}
