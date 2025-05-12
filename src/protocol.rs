use bincode::{Decode, Encode};

/// Client request
#[derive(Encode, Decode)]
pub enum Request {
    /// Upload a path
    Upload(Upload),
}

/// Server request
#[derive(Encode, Decode)]
pub enum Response {
    /// Response for `Request::Upload`
    Upload,
}

/// Contents of compile request
#[derive(Encode, Decode)]
pub struct Upload {
    /// The store path to upload
    pub path: String,
}
