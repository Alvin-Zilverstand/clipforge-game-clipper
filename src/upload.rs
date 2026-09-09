use crate::models::UploadProvider;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadMetadata {
    pub clip_id: String,
    pub game_id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadResult {
    pub provider: UploadProvider,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthField {
    pub name: String,
    pub required: bool,
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploaderInfo {
    pub name: String,
    pub provider: UploadProvider,
    pub max_file_size_mb: Option<u64>,
    pub auth_schema: Vec<AuthField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadError {
    NotConfigured(String),
    FileTooLarge { max_mb: u64 },
    Network(String),
    Provider(String),
}

pub trait Uploader {
    fn info(&self) -> UploaderInfo;
    fn upload(&self, file: &Path, metadata: &UploadMetadata) -> Result<UploadResult, UploadError>;
}

#[derive(Debug, Clone, Default)]
pub struct CatboxUploader {
    pub userhash: Option<String>,
}

impl Uploader for CatboxUploader {
    fn info(&self) -> UploaderInfo {
        UploaderInfo {
            name: "Catbox".to_string(),
            provider: UploadProvider::Catbox,
            max_file_size_mb: Some(200),
            auth_schema: vec![AuthField {
                name: "userhash".to_string(),
                required: false,
                secret: true,
            }],
        }
    }

    fn upload(&self, file: &Path, _metadata: &UploadMetadata) -> Result<UploadResult, UploadError> {
        ensure_file_limit(file, 200)?;
        Err(UploadError::Network(
            "Catbox upload is wired at the adapter boundary; enable HTTP client dependency for live uploads."
                .to_string(),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct LitterboxUploader {
    pub expiry_hours: u8,
}

impl Default for LitterboxUploader {
    fn default() -> Self {
        Self { expiry_hours: 24 }
    }
}

impl Uploader for LitterboxUploader {
    fn info(&self) -> UploaderInfo {
        UploaderInfo {
            name: "Litterbox".to_string(),
            provider: UploadProvider::Litterbox,
            max_file_size_mb: Some(1_000),
            auth_schema: Vec::new(),
        }
    }

    fn upload(&self, file: &Path, _metadata: &UploadMetadata) -> Result<UploadResult, UploadError> {
        ensure_file_limit(file, 1_000)?;
        if !matches!(self.expiry_hours, 1 | 12 | 24 | 72) {
            return Err(UploadError::Provider(
                "Litterbox expiry must be one of 1, 12, 24, or 72 hours.".to_string(),
            ));
        }
        Err(UploadError::Network(
            "Litterbox upload is wired at the adapter boundary; enable HTTP client dependency for live uploads."
                .to_string(),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct CustomHttpUploader {
    pub endpoint: String,
    pub method: String,
    pub multipart_field: String,
    pub response_url_path: String,
}

impl Uploader for CustomHttpUploader {
    fn info(&self) -> UploaderInfo {
        UploaderInfo {
            name: "Custom HTTP".to_string(),
            provider: UploadProvider::CustomHttp,
            max_file_size_mb: None,
            auth_schema: vec![AuthField {
                name: "headers".to_string(),
                required: false,
                secret: true,
            }],
        }
    }

    fn upload(
        &self,
        _file: &Path,
        _metadata: &UploadMetadata,
    ) -> Result<UploadResult, UploadError> {
        if self.endpoint.trim().is_empty() {
            return Err(UploadError::NotConfigured(
                "custom upload endpoint is required".to_string(),
            ));
        }
        Err(UploadError::Network(
            "Custom upload is configured; enable HTTP client dependency for live uploads."
                .to_string(),
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub struct LustfulUploader;

impl Uploader for LustfulUploader {
    fn info(&self) -> UploaderInfo {
        UploaderInfo {
            name: "Lustful".to_string(),
            provider: UploadProvider::Lustful,
            max_file_size_mb: None,
            auth_schema: Vec::new(),
        }
    }

    fn upload(
        &self,
        _file: &Path,
        _metadata: &UploadMetadata,
    ) -> Result<UploadResult, UploadError> {
        Err(UploadError::NotConfigured(
            "Lustful API details must be confirmed before live uploads are enabled.".to_string(),
        ))
    }
}

fn ensure_file_limit(file: &Path, max_mb: u64) -> Result<(), UploadError> {
    let Ok(metadata) = file.metadata() else {
        return Ok(());
    };
    if metadata.len() > max_mb * 1024 * 1024 {
        return Err(UploadError::FileTooLarge { max_mb });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn litterbox_rejects_invalid_expiry() {
        let uploader = LitterboxUploader { expiry_hours: 8 };
        let result = uploader.upload(
            Path::new("missing.mp4"),
            &UploadMetadata {
                clip_id: "clip".to_string(),
                game_id: "game".to_string(),
                title: "Clip".to_string(),
            },
        );

        assert!(matches!(result, Err(UploadError::Provider(_))));
    }
}
