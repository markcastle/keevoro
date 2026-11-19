//! Storage in an S3-compatible bucket.
//!
//! One adapter covers Cloudflare R2, Amazon S3, MinIO and the many other
//! services that speak the same protocol. What differs between them (the
//! address, the region, how the bucket name is put into the address) is
//! captured by a [`Preset`], so the user picks a provider and pastes two
//! keys, and never meets the word "endpoint".
//!
//! Things worth knowing before changing this file:
//!
//! - **Write-once is enforced by the server.** `create_only` sends
//!   `If-None-Match: *`, so the bucket itself refuses to replace an
//!   original even if two computers upload at once.
//! - **Extra checksums are off unless required.** Newer AWS SDKs add
//!   checksum headers by default, and several S3-compatible servers reject
//!   them. Keevoro verifies content itself with BLAKE3.
//! - **An upload id here is more than S3's id.** S3 needs the object key
//!   with every multipart call, but the storage trait passes only the id,
//!   so the key (and whether the upload is write-once) is packed into it.
//!   See [`PackedUploadId`].
//! - **Secrets are passed in, never read from disk here.** The caller
//!   fetches them from the system keychain or the environment.
//!
//! This crate is checked by the shared conformance suite against a real
//! S3-compatible server (MinIO in CI). Without a server configured, only
//! the pure parts are tested; see `tests.rs` for the variables to set.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use async_trait::async_trait;
use aws_sdk_s3::config::{
    BehaviorVersion, Credentials, Region, RequestChecksumCalculation, ResponseChecksumValidation,
};
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::Client;
use keevoro_storage::{
    ByteSource, Capabilities, ConnectionReport, ListPage, MultipartLimits, MultipartStore, ObjectInfo, ObjectKey,
    ObjectStore, PartReceipt, PutOptions, PutReceipt, StorageError, UploadId,
};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

const MIB: u64 = 1024 * 1024;

/// Which service the bucket lives on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preset {
    /// Cloudflare R2. The account id is the 32-character code shown on the
    /// R2 overview page.
    CloudflareR2 { account_id: String },
    /// Amazon S3, in a region such as `eu-west-2`.
    AmazonS3 { region: String },
    /// MinIO, a NAS, or any other S3-compatible server at a known address.
    Custom {
        endpoint: String,
        region: Option<String>,
        /// Put the bucket in the path (`host/bucket/key`) instead of the
        /// host name. Self-hosted servers usually need this.
        path_style: bool,
    },
}

/// Everything needed to reach one bucket.
#[derive(Clone)]
pub struct S3Settings {
    pub preset: Preset,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

/// The connection details a preset works out to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Resolved {
    endpoint: Option<String>,
    region: String,
    path_style: bool,
}

impl Preset {
    fn resolve(&self) -> Result<Resolved, StorageError> {
        let bad = |what: &str| StorageError::Provider(what.to_string());
        match self {
            Self::CloudflareR2 { account_id } => {
                let id = account_id.trim().to_lowercase();
                if id.len() != 32 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(bad(
                        "That doesn't look like a Cloudflare account ID. It is 32 letters and numbers",
                    ));
                }
                Ok(Resolved {
                    endpoint: Some(format!("https://{id}.r2.cloudflarestorage.com")),
                    region: "auto".into(),
                    path_style: false,
                })
            }
            Self::AmazonS3 { region } => {
                let region = region.trim().to_lowercase();
                if region.is_empty() || !region.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    return Err(bad("That doesn't look like an Amazon region. They look like eu-west-2"));
                }
                Ok(Resolved {
                    endpoint: None,
                    region,
                    path_style: false,
                })
            }
            Self::Custom {
                endpoint,
                region,
                path_style,
            } => {
                let endpoint = endpoint.trim().trim_end_matches('/').to_string();
                if !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) {
                    return Err(bad("The storage address should start with https://"));
                }
                Ok(Resolved {
                    endpoint: Some(endpoint),
                    region: region
                        .clone()
                        .filter(|r| !r.trim().is_empty())
                        .unwrap_or_else(|| "us-east-1".into()),
                    path_style: *path_style,
                })
            }
        }
    }
}

pub struct S3Store {
    client: Client,
    /// The same bucket with no keys, to see what a stranger can see.
    anonymous: Client,
    bucket: String,
    name: String,
    caps: Capabilities,
}

impl S3Store {
    /// Prepares a connection. Nothing is sent until the first request, so
    /// call [`ObjectStore::check_connection`] to find out whether the keys
    /// work.
    pub fn connect(settings: &S3Settings) -> Result<Self, StorageError> {
        let resolved = settings.preset.resolve()?;
        let bucket = settings.bucket.trim().to_string();
        if bucket.is_empty() || bucket.contains('/') || bucket.contains(char::is_whitespace) {
            return Err(StorageError::Provider(
                "That doesn't look like a bucket name. It is a single word with no spaces".into(),
            ));
        }
        let base = || {
            let mut b = aws_sdk_s3::Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .region(Region::new(resolved.region.clone()))
                .force_path_style(resolved.path_style)
                .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
                .response_checksum_validation(ResponseChecksumValidation::WhenRequired);
            if let Some(url) = &resolved.endpoint {
                b = b.endpoint_url(url);
            }
            b
        };
        let credentials = Credentials::new(
            settings.access_key_id.trim(),
            settings.secret_access_key.trim(),
            None,
            None,
            "keevoro",
        );
        let name = format!(
            "s3:{}/{bucket}",
            resolved
                .endpoint
                .clone()
                .unwrap_or_else(|| format!("aws:{}", resolved.region))
        );
        Ok(Self {
            client: Client::from_conf(base().credentials_provider(credentials).build()),
            anonymous: Client::from_conf(base().allow_no_auth().build()),
            bucket,
            name,
            caps: Capabilities {
                multipart: Some(MultipartLimits {
                    min_part_bytes: 5 * MIB,
                    max_part_bytes: 5 * 1024 * MIB,
                    max_parts: 10_000,
                }),
                conditional_create: true,
                server_checksums: false,
                custom_metadata: true,
                whole_files_preferred: false,
                counts_its_own_bytes: false,
            },
        })
    }

    /// A stable name for this bucket, used to keep upload records for
    /// different buckets apart. Contains no secrets.
    pub fn target_name(&self) -> &str {
        &self.name
    }
}

/// S3's upload id together with the key it belongs to and whether the
/// finished object must not replace an existing one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PackedUploadId {
    create_only: bool,
    upload_id: String,
    key: String,
}

impl PackedUploadId {
    fn pack(&self) -> UploadId {
        // S3 upload ids never contain a space, so the first one splits.
        UploadId(format!("{}{} {}", u8::from(self.create_only), self.upload_id, self.key))
    }

    fn unpack(id: &UploadId) -> Result<Self, StorageError> {
        let gone = || StorageError::UploadNotFound(id.0.clone());
        let (head, key) = id.0.split_once(' ').ok_or_else(gone)?;
        let (flag, upload_id) = head.split_at_checked(1).ok_or_else(gone)?;
        if upload_id.is_empty() || key.is_empty() || !matches!(flag, "0" | "1") {
            return Err(gone());
        }
        Ok(Self {
            create_only: flag == "1",
            upload_id: upload_id.to_string(),
            key: key.to_string(),
        })
    }
}

/// Turns the server's answer into one of keevoro's error kinds. `what` is
/// the key or upload the request was about.
fn classify(code: Option<&str>, status: Option<u16>, what: &str, detail: String) -> StorageError {
    match (code.unwrap_or_default(), status.unwrap_or(0)) {
        ("NoSuchUpload", _) => StorageError::UploadNotFound(what.to_string()),
        ("NoSuchKey" | "NotFound" | "NoSuchBucket", _) | (_, 404) => StorageError::NotFound(what.to_string()),
        ("PreconditionFailed" | "ConditionalRequestConflict", _) | (_, 412 | 409) => {
            StorageError::AlreadyExists(what.to_string())
        }
        ("AccessDenied" | "InvalidAccessKeyId" | "SignatureDoesNotMatch" | "Unauthorized", _) | (_, 401 | 403) => {
            StorageError::AccessDenied(detail)
        }
        ("InvalidPart" | "InvalidPartOrder" | "EntityTooSmall" | "MalformedXML", _) => {
            StorageError::InvalidParts(detail)
        }
        // The server is busy or briefly unwell: worth another try.
        ("SlowDown" | "RequestTimeout" | "InternalError" | "ServiceUnavailable", _) | (_, 429 | 500..=599) => {
            StorageError::Network(detail)
        }
        _ => StorageError::Provider(detail),
    }
}

fn map_err<E>(e: SdkError<E>, what: &str) -> StorageError
where
    E: ProvideErrorMetadata + std::fmt::Debug,
{
    match &e {
        SdkError::DispatchFailure(_) | SdkError::TimeoutError(_) | SdkError::ResponseError(_) => {
            StorageError::Network(format!("{e:?}"))
        }
        SdkError::ServiceError(service) => {
            let status = service.raw().status().as_u16();
            let detail = format!(
                "{} ({status})",
                e.message().or(e.code()).unwrap_or("the storage gave no reason")
            );
            classify(e.code(), Some(status), what, detail)
        }
        _ => StorageError::Provider(format!("{e:?}")),
    }
}

async fn body_of(source: ByteSource) -> Result<(ByteStream, u64), StorageError> {
    match source {
        ByteSource::Bytes(b) => {
            let len = b.len() as u64;
            Ok((ByteStream::from(b), len))
        }
        ByteSource::File(path) => {
            let len = std::fs::metadata(&path)?.len();
            let stream = ByteStream::from_path(&path)
                .await
                .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;
            Ok((stream, len))
        }
    }
}

fn to_u64(n: Option<i64>) -> u64 {
    n.and_then(|n| u64::try_from(n).ok()).unwrap_or(0)
}

/// Parts must be numbered 1, 2, 3 with none missing. S3 itself accepts
/// gaps, which would silently produce a file with a hole in it.
fn check_parts(parts: &[PartReceipt]) -> Result<(), StorageError> {
    if parts.is_empty() {
        return Err(StorageError::InvalidParts("no parts".into()));
    }
    for (i, p) in parts.iter().enumerate() {
        if usize::from(p.number) != i + 1 {
            return Err(StorageError::InvalidParts(format!(
                "expected part {} but got part {}",
                i + 1,
                p.number
            )));
        }
    }
    Ok(())
}

#[async_trait]
impl ObjectStore for S3Store {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn check_connection(&self) -> Result<ConnectionReport, StorageError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let probe = ObjectKey::new(format!(".keevoro-check/{nanos}"))?;
        self.put(&probe, ByteSource::Bytes(b"keevoro".to_vec()), &PutOptions::default())
            .await?;
        let can_read = self.get(&probe).await.is_ok_and(|b| b == b"keevoro");
        self.delete(&probe).await?;

        // Can someone with no keys at all list the bucket?
        let stranger = self
            .anonymous
            .list_objects_v2()
            .bucket(&self.bucket)
            .max_keys(1)
            .send()
            .await;
        let publicly_listable = match stranger {
            Ok(_) => Some(true),
            Err(e) => match map_err(e, &self.bucket) {
                StorageError::AccessDenied(_) => Some(false),
                _ => None,
            },
        };
        Ok(ConnectionReport {
            can_write: true,
            can_read,
            publicly_listable,
        })
    }

    async fn head(&self, key: &ObjectKey) -> Result<Option<ObjectInfo>, StorageError> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key.as_str())
            .send()
            .await
        {
            Ok(found) => Ok(Some(ObjectInfo {
                key: key.clone(),
                bytes: to_u64(found.content_length()),
                metadata: found
                    .metadata()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default(),
            })),
            Err(e) => match map_err(e, key.as_str()) {
                StorageError::NotFound(_) => Ok(None),
                other => Err(other),
            },
        }
    }

    async fn get(&self, key: &ObjectKey) -> Result<Vec<u8>, StorageError> {
        let found = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key.as_str())
            .send()
            .await
            .map_err(|e| map_err(e, key.as_str()))?;
        let bytes = found
            .body
            .collect()
            .await
            .map_err(|e| StorageError::Network(e.to_string()))?;
        Ok(bytes.into_bytes().to_vec())
    }

    async fn put(&self, key: &ObjectKey, body: ByteSource, opts: &PutOptions) -> Result<PutReceipt, StorageError> {
        let (stream, bytes) = body_of(body).await?;
        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key.as_str())
            .body(stream)
            .set_content_type(opts.content_type.clone())
            .set_metadata(Some(opts.metadata.clone().into_iter().collect()));
        if opts.create_only {
            request = request.if_none_match("*");
        }
        let done = request.send().await.map_err(|e| map_err(e, key.as_str()))?;
        Ok(PutReceipt {
            key: key.clone(),
            bytes,
            etag: done.e_tag().map(str::to_string),
            blake3: None,
        })
    }

    async fn list_page(&self, prefix: &str, continuation: Option<String>) -> Result<ListPage, StorageError> {
        let page = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(prefix)
            .set_continuation_token(continuation)
            .send()
            .await
            .map_err(|e| map_err(e, prefix))?;
        let objects = page
            .contents()
            .iter()
            // Something else may have put oddly named objects in the
            // bucket; they are not keevoro's and are passed over.
            .filter_map(|o| {
                Some(ObjectInfo {
                    key: ObjectKey::new(o.key()?).ok()?,
                    bytes: to_u64(o.size()),
                    metadata: BTreeMap::new(),
                })
            })
            .collect();
        Ok(ListPage {
            objects,
            next: page.next_continuation_token().map(str::to_string),
        })
    }

    async fn delete(&self, key: &ObjectKey) -> Result<(), StorageError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key.as_str())
            .send()
            .await
            .map_err(|e| map_err(e, key.as_str()))?;
        Ok(())
    }
}

#[async_trait]
impl MultipartStore for S3Store {
    async fn start(&self, key: &ObjectKey, opts: &PutOptions) -> Result<UploadId, StorageError> {
        let started = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(key.as_str())
            .set_content_type(opts.content_type.clone())
            .set_metadata(Some(opts.metadata.clone().into_iter().collect()))
            .send()
            .await
            .map_err(|e| map_err(e, key.as_str()))?;
        let upload_id = started
            .upload_id()
            .ok_or_else(|| StorageError::Provider("the storage did not return an upload id".into()))?;
        Ok(PackedUploadId {
            create_only: opts.create_only,
            upload_id: upload_id.to_string(),
            key: key.to_string(),
        }
        .pack())
    }

    async fn put_part(&self, id: &UploadId, number: u16, body: ByteSource) -> Result<PartReceipt, StorageError> {
        let packed = PackedUploadId::unpack(id)?;
        if number == 0 {
            return Err(StorageError::InvalidParts("part numbers start at 1".into()));
        }
        let (stream, bytes) = body_of(body).await?;
        let sent = self
            .client
            .upload_part()
            .bucket(&self.bucket)
            .key(&packed.key)
            .upload_id(&packed.upload_id)
            .part_number(i32::from(number))
            .body(stream)
            .send()
            .await
            .map_err(|e| map_err(e, &id.0))?;
        Ok(PartReceipt {
            number,
            bytes,
            etag: sent.e_tag().unwrap_or_default().to_string(),
        })
    }

    async fn complete(&self, id: &UploadId, parts: &[PartReceipt]) -> Result<PutReceipt, StorageError> {
        let packed = PackedUploadId::unpack(id)?;
        check_parts(parts)?;
        let list = parts
            .iter()
            .map(|p| {
                CompletedPart::builder()
                    .part_number(i32::from(p.number))
                    .e_tag(&p.etag)
                    .build()
            })
            .collect();
        let mut request = self
            .client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(&packed.key)
            .upload_id(&packed.upload_id)
            .multipart_upload(CompletedMultipartUpload::builder().set_parts(Some(list)).build());
        if packed.create_only {
            request = request.if_none_match("*");
        }
        let done = request.send().await.map_err(|e| map_err(e, &packed.key))?;
        Ok(PutReceipt {
            key: ObjectKey::new(packed.key)?,
            bytes: parts.iter().map(|p| p.bytes).sum(),
            etag: done.e_tag().map(str::to_string),
            blake3: None,
        })
    }

    async fn abort(&self, id: &UploadId) -> Result<(), StorageError> {
        let packed = PackedUploadId::unpack(id)?;
        self.client
            .abort_multipart_upload()
            .bucket(&self.bucket)
            .key(&packed.key)
            .upload_id(&packed.upload_id)
            .send()
            .await
            .map_err(|e| map_err(e, &id.0))?;
        Ok(())
    }

    async fn list_parts(&self, id: &UploadId) -> Result<Vec<PartReceipt>, StorageError> {
        let packed = PackedUploadId::unpack(id)?;
        let mut parts = Vec::new();
        let mut marker: Option<String> = None;
        loop {
            let page = self
                .client
                .list_parts()
                .bucket(&self.bucket)
                .key(&packed.key)
                .upload_id(&packed.upload_id)
                .set_part_number_marker(marker.clone())
                .send()
                .await
                .map_err(|e| map_err(e, &id.0))?;
            for p in page.parts() {
                if let Some(number) = p.part_number().and_then(|n| u16::try_from(n).ok()) {
                    parts.push(PartReceipt {
                        number,
                        bytes: to_u64(p.size()),
                        etag: p.e_tag().unwrap_or_default().to_string(),
                    });
                }
            }
            match (page.is_truncated(), page.next_part_number_marker()) {
                (Some(true), Some(next)) if Some(next) != marker.as_deref() => marker = Some(next.to_string()),
                _ => break,
            }
        }
        parts.sort_by_key(|p| p.number);
        Ok(parts)
    }
}

#[cfg(test)]
mod tests;
