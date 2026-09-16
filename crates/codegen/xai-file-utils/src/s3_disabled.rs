//! Stub S3 surface when feature `cloud-upload` is off.
//!
//! Keeps the public module path and types used by tools (e.g. video_gen)
//! so upstream call sites still typecheck. Operations return a clear error
//! instead of linking `aws-sdk-s3`.

use std::path::Path;

/// Static access-key credentials for presigning S3 URLs.
///
/// `Debug` is intentionally redacted — the struct holds plaintext secrets.
#[derive(Clone)]
pub struct S3StaticCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl std::fmt::Debug for S3StaticCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3StaticCredentials")
            .field("access_key_id", &"[redacted]")
            .field("secret_access_key", &"[redacted]")
            .finish()
    }
}

fn not_compiled() -> anyhow::Error {
    anyhow::anyhow!("S3 support is not compiled into this build (missing feature `cloud-upload`)")
}

pub async fn presign_put_url(
    _region: &str,
    _endpoint_url: Option<&str>,
    _creds: &S3StaticCredentials,
    _bucket: &str,
    _key: &str,
    _content_type: &str,
    _expires_in: std::time::Duration,
) -> anyhow::Result<String> {
    Err(not_compiled())
}

pub async fn presign_get_url(
    _region: &str,
    _endpoint_url: Option<&str>,
    _creds: &S3StaticCredentials,
    _bucket: &str,
    _key: &str,
    _expires_in: std::time::Duration,
) -> anyhow::Result<String> {
    Err(not_compiled())
}

pub async fn upload_bytes(
    _bucket: &str,
    _object_path: &str,
    _content: &[u8],
    _content_type: &str,
    _region: &str,
    _credentials_content: Option<&str>,
    _credentials_file: Option<&str>,
    _endpoint_url: Option<&str>,
) -> anyhow::Result<String> {
    Err(not_compiled())
}

pub async fn upload_file(
    _bucket: &str,
    _object_path: &str,
    _file_path: &Path,
    _content_type: &str,
    _region: &str,
    _credentials_content: Option<&str>,
    _credentials_file: Option<&str>,
    _endpoint_url: Option<&str>,
) -> anyhow::Result<String> {
    Err(not_compiled())
}

pub async fn upload_stream<R: tokio::io::AsyncRead + Send + Sync + 'static>(
    _bucket: &str,
    _object_path: &str,
    _reader: R,
    _content_type: &str,
    _region: &str,
    _credentials_content: Option<&str>,
    _credentials_file: Option<&str>,
    _endpoint_url: Option<&str>,
) -> anyhow::Result<String> {
    Err(not_compiled())
}
