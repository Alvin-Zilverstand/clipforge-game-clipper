use crate::models::UploadProvider;
use std::path::Path;
use std::time::Duration;

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

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured(message) => write!(f, "{message}"),
            Self::FileTooLarge { max_mb } => write!(f, "file is larger than {max_mb} MB"),
            Self::Network(message) => write!(f, "upload network error: {message}"),
            Self::Provider(message) => write!(f, "upload provider error: {message}"),
        }
    }
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
        let client = upload_client()?;
        let file_part = reqwest::blocking::multipart::Part::file(file)
            .map_err(|error| UploadError::Network(error.to_string()))?;
        let mut form = reqwest::blocking::multipart::Form::new()
            .text("reqtype", "fileupload")
            .part("fileToUpload", file_part);
        if let Some(userhash) = &self.userhash {
            if !userhash.trim().is_empty() {
                form = form.text("userhash", userhash.clone());
            }
        }
        let body = client
            .post("https://catbox.moe/user/api.php")
            .multipart(form)
            .send()
            .map_err(|error| UploadError::Network(error.to_string()))?
            .error_for_status()
            .map_err(|error| UploadError::Provider(error.to_string()))?
            .text()
            .map_err(|error| UploadError::Network(error.to_string()))?;
        let url = parse_plain_url(&body)?;
        Ok(UploadResult {
            provider: UploadProvider::Catbox,
            url,
        })
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
        let client = upload_client()?;
        let file_part = reqwest::blocking::multipart::Part::file(file)
            .map_err(|error| UploadError::Network(error.to_string()))?;
        let form = reqwest::blocking::multipart::Form::new()
            .text("reqtype", "fileupload")
            .text("time", format!("{}h", self.expiry_hours))
            .part("fileToUpload", file_part);
        let body = client
            .post("https://litterbox.catbox.moe/resources/internals/api.php")
            .multipart(form)
            .send()
            .map_err(|error| UploadError::Network(error.to_string()))?
            .error_for_status()
            .map_err(|error| UploadError::Provider(error.to_string()))?
            .text()
            .map_err(|error| UploadError::Network(error.to_string()))?;
        let url = parse_plain_url(&body)?;
        Ok(UploadResult {
            provider: UploadProvider::Litterbox,
            url,
        })
    }
}

#[derive(Debug, Clone)]
pub struct CustomHttpUploader {
    pub endpoint: String,
    pub method: String,
    pub multipart_field: String,
    pub response_url_path: String,
    pub headers: Vec<(String, String)>,
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

    fn upload(&self, file: &Path, metadata: &UploadMetadata) -> Result<UploadResult, UploadError> {
        if self.endpoint.trim().is_empty() {
            return Err(UploadError::NotConfigured(
                "custom upload endpoint is required".to_string(),
            ));
        }
        if !self.method.eq_ignore_ascii_case("POST") {
            return Err(UploadError::Provider(
                "custom uploads currently support POST multipart requests".to_string(),
            ));
        }
        let client = upload_client()?;
        let file_part = reqwest::blocking::multipart::Part::file(file)
            .map_err(|error| UploadError::Network(error.to_string()))?;
        let field_name = if self.multipart_field.trim().is_empty() {
            "file"
        } else {
            self.multipart_field.trim()
        };
        let form = reqwest::blocking::multipart::Form::new()
            .text("clip_id", metadata.clip_id.clone())
            .text("game_id", metadata.game_id.clone())
            .text("title", metadata.title.clone())
            .part(field_name.to_string(), file_part);
        let mut request = client.post(&self.endpoint).multipart(form);
        for (name, value) in &self.headers {
            if !name.trim().is_empty() {
                request = request.header(name, value);
            }
        }
        let body = request
            .send()
            .map_err(|error| UploadError::Network(error.to_string()))?
            .error_for_status()
            .map_err(|error| UploadError::Provider(error.to_string()))?
            .text()
            .map_err(|error| UploadError::Network(error.to_string()))?;
        let url = if self.response_url_path.trim().is_empty() {
            parse_plain_url(&body)?
        } else {
            extract_json_path(&body, &self.response_url_path)?
        };
        Ok(UploadResult {
            provider: UploadProvider::CustomHttp,
            url,
        })
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

fn upload_client() -> Result<reqwest::blocking::Client, UploadError> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent("ClipForge/0.1")
        .build()
        .map_err(|error| UploadError::Network(error.to_string()))
}

fn parse_plain_url(body: &str) -> Result<String, UploadError> {
    let trimmed = body.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Ok(trimmed.to_string())
    } else {
        Err(UploadError::Provider(trimmed.to_string()))
    }
}

fn extract_json_path(body: &str, path: &str) -> Result<String, UploadError> {
    let mut value: &serde_json::Value = &serde_json::from_str(body)
        .map_err(|error| UploadError::Provider(format!("invalid JSON response: {error}")))?;
    for key in path.trim().trim_start_matches("$.").split('.') {
        if key.is_empty() {
            continue;
        }
        value = value.get(key).ok_or_else(|| {
            UploadError::Provider(format!("response URL path was not found: {path}"))
        })?;
    }
    let Some(url) = value.as_str() else {
        return Err(UploadError::Provider(format!(
            "response URL path is not a string: {path}"
        )));
    };
    parse_plain_url(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{ErrorKind, Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

    #[test]
    fn extracts_custom_json_url_path() {
        let url = extract_json_path(
            r#"{"data":{"url":"https://example.test/c.mp4"}}"#,
            "data.url",
        )
        .expect("url");
        assert_eq!(url, "https://example.test/c.mp4");
    }

    #[test]
    fn custom_http_uploader_sends_multipart_file() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local upload test");
        let endpoint = format!(
            "http://{}",
            listener.local_addr().expect("local upload address")
        );
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept upload request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set upload read timeout");
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        request.extend_from_slice(&buffer[..read]);
                        if request_body_is_complete(&request) {
                            break;
                        }
                    }
                    Err(error)
                        if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                    {
                        break;
                    }
                    Err(error) => panic!("read upload request: {error}"),
                }
            }
            let request_text = String::from_utf8_lossy(&request);
            assert!(request_text.contains("POST / HTTP/1.1"));
            assert!(request_text.contains("multipart/form-data"));
            assert!(request_text.contains("clip_id"));
            assert!(request_text.contains("clipforge-bytes"));
            let body = r#"{"data":{"url":"https://uploads.example.test/clip.mp4"}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write upload response");
        });

        let file_path = std::env::temp_dir().join(format!(
            "clipforge-upload-{}.txt",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        std::fs::write(&file_path, b"clipforge-bytes").expect("write upload test file");
        let uploader = CustomHttpUploader {
            endpoint,
            method: "POST".to_string(),
            multipart_field: "file".to_string(),
            response_url_path: "data.url".to_string(),
            headers: Vec::new(),
        };

        let result = uploader
            .upload(
                &file_path,
                &UploadMetadata {
                    clip_id: "clip".to_string(),
                    game_id: "game".to_string(),
                    title: "Clip".to_string(),
                },
            )
            .expect("custom HTTP upload result");

        assert_eq!(result.url, "https://uploads.example.test/clip.mp4");
        server.join().expect("upload test server");
        let _ = std::fs::remove_file(file_path);
    }

    fn request_body_is_complete(request: &[u8]) -> bool {
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            return false;
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        });
        content_length
            .map(|length| request.len() >= header_end + 4 + length)
            .unwrap_or_else(|| {
                request
                    .windows(17)
                    .any(|window| window == b"clipforge-bytes")
            })
    }
}
