extern crate base64;
extern crate md5;

use std::io::Write;

use attohttpc::header::{HeaderName};

use super::bucket::Bucket;
use super::command::Command;
use chrono::{DateTime, Utc};

use crate::command::HttpMethod;
use crate::request_trait::Request;
use crate::{Result, S3Error};

// static CLIENT: Lazy<Client> = Lazy::new(|| {
//     if cfg!(feature = "no-verify-ssl") {
//         Client::builder()
//             .danger_accept_invalid_certs(true)
//             .danger_accept_invalid_hostnames(true)
//             .build()
//             .expect("Could not build dangerous client!")
//     } else {
//         Client::new()
//     }
// });

impl std::convert::From<attohttpc::Error> for S3Error {
    fn from(e: attohttpc::Error) -> S3Error {
        S3Error {
            description: Some(format!("{}", e)),
            data: None,
            source: None,
        }
    }
}

impl std::convert::From<http::header::InvalidHeaderValue> for S3Error {
    fn from(e: http::header::InvalidHeaderValue) -> S3Error {
        S3Error {
            description: Some(format!("{}", e)),
            data: None,
            source: None,
        }
    }
}

// Temporary structure for making a request
pub struct AttoRequest<'a> {
    pub bucket: &'a Bucket,
    pub path: &'a str,
    pub command: Command<'a>,
    pub datetime: DateTime<Utc>,
    pub sync: bool,
}

impl<'a> Request for AttoRequest<'a> {
    type Response = attohttpc::Response;
    type HeaderMap = attohttpc::header::HeaderMap;

    fn datetime(&self) -> DateTime<Utc> {
        self.datetime
    }

    fn bucket(&self) -> Bucket {
        self.bucket.clone()
    }

    fn command(&self) -> Command {
        self.command.clone()
    }

    fn path(&self) -> String {
        self.path.to_string()
    }

    fn response(&self) -> Result<Self::Response> {
        // Build headers
        let headers = match self.headers() {
            Ok(headers) => headers,
            Err(e) => return Err(e),
        };

        // Get owned content to pass to reqwest
        let content = if let Command::PutObject { content, .. } = self.command {
            Vec::from(content)
        } else if let Command::PutObjectTagging { tags } = self.command {
            Vec::from(tags)
        } else if let Command::UploadPart { content, .. } = self.command {
            Vec::from(content)
        } else if let Command::CompleteMultipartUpload { data, .. } = &self.command {
            let body = data.to_string();
            // assert_eq!(body, "body".to_string());
            body.as_bytes().to_vec()
        } else {
            Vec::new()
        };

        let mut session = attohttpc::Session::new();

        for (name, value) in headers {
            session.header(HeaderName::from_bytes(name.as_bytes()).unwrap(), value);
        }

        let request = match self.command.http_verb() {
            HttpMethod::Get => session.get(self.url(false)),
            HttpMethod::Delete => session.delete(self.url(false)),
            HttpMethod::Put => session.put(self.url(false)),
            HttpMethod::Post => session.post(self.url(false)),
            HttpMethod::Head => session.head(self.url(false)),
        };

        let response = request.bytes(&content).send()?;

        // let response = request.send()?;

        if cfg!(feature = "fail-on-err") && response.status().as_u16() >= 400 {
            return Err(S3Error::from(
                format!(
                    "Request failed with code {}\n{}",
                    response.status().as_u16(),
                    response.text()?
                )
                .as_str(),
            ));
        }

        Ok(response)
    }

    fn response_data(&self, etag: bool) -> Result<(Vec<u8>, u16)> {
        let response = self.response()?;
        let status_code = response.status().as_u16();
        let headers = response.headers().clone();
        let etag_header = headers.get("ETag");
        let body = response.bytes()?;
        let mut body_vec = Vec::new();
        body_vec.extend_from_slice(&body[..]);
        if etag {
            if let Some(etag) = etag_header {
                body_vec = etag.to_str()?.as_bytes().to_vec();
            }
        }
        Ok((body_vec, status_code))
    }

    fn response_data_to_writer<'b, T: Write>(&self, writer: &'b mut T) -> Result<u16> {
        let response = self.response()?;

        let status_code = response.status();
        let stream = response.bytes()?;

        writer.write_all(&stream)?;

        Ok(status_code.as_u16())
    }

    fn response_header(&self) -> Result<(Self::HeaderMap, u16)> {
        let response = self.response()?;
        let status_code = response.status().as_u16();
        let headers = response.headers().clone();
        Ok((headers, status_code))
    }
}

impl<'a> AttoRequest<'a> {
    pub fn new<'b>(bucket: &'b Bucket, path: &'b str, command: Command<'b>) -> AttoRequest<'b> {
        AttoRequest {
            bucket,
            path,
            command,
            datetime: Utc::now(),
            sync: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::blocking::AttoRequest;
    use crate::bucket::Bucket;
    use crate::command::Command;
    use crate::request_trait::Request;
    use crate::Result;
    use awscreds::Credentials;

    // Fake keys - otherwise using Credentials::default will use actual user
    // credentials if they exist.
    fn fake_credentials() -> Credentials {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secert_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        Credentials::new(Some(access_key), Some(secert_key), None, None, None).unwrap()
    }

    #[test]
    fn url_uses_https_by_default() -> Result<()> {
        let region = "custom-region".parse()?;
        let bucket = Bucket::new("my-first-bucket", region, fake_credentials())?;
        let path = "/my-first/path";
        let request = AttoRequest::new(&bucket, path, Command::GetObject);

        assert_eq!(request.url(false).scheme(), "https");

        let headers = request.headers().unwrap();
        let host = headers.get("Host").unwrap();

        assert_eq!(*host, "my-first-bucket.custom-region".to_string());
        Ok(())
    }

    #[test]
    fn url_uses_https_by_default_path_style() -> Result<()> {
        let region = "custom-region".parse()?;
        let bucket = Bucket::new_with_path_style("my-first-bucket", region, fake_credentials())?;
        let path = "/my-first/path";
        let request = AttoRequest::new(&bucket, path, Command::GetObject);

        assert_eq!(request.url(false).scheme(), "https");

        let headers = request.headers().unwrap();
        let host = headers.get("Host").unwrap();

        assert_eq!(*host, "custom-region".to_string());
        Ok(())
    }

    #[test]
    fn url_uses_scheme_from_custom_region_if_defined() -> Result<()> {
        let region = "http://custom-region".parse()?;
        let bucket = Bucket::new("my-second-bucket", region, fake_credentials())?;
        let path = "/my-second/path";
        let request = AttoRequest::new(&bucket, path, Command::GetObject);

        assert_eq!(request.url(false).scheme(), "http");

        let headers = request.headers().unwrap();
        let host = headers.get("Host").unwrap();
        assert_eq!(*host, "my-second-bucket.custom-region".to_string());
        Ok(())
    }

    #[test]
    fn url_uses_scheme_from_custom_region_if_defined_with_path_style() -> Result<()> {
        let region = "http://custom-region".parse()?;
        let bucket = Bucket::new_with_path_style("my-second-bucket", region, fake_credentials())?;
        let path = "/my-second/path";
        let request = AttoRequest::new(&bucket, path, Command::GetObject);

        assert_eq!(request.url(false).scheme(), "http");

        let headers = request.headers().unwrap();
        let host = headers.get("Host").unwrap();
        assert_eq!(*host, "custom-region".to_string());

        Ok(())
    }
}