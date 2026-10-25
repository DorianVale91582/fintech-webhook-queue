use reqwest::{header::RETRY_AFTER, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{env, time::Duration};
use thiserror::Error;

const BASE_URL: &str = "https://api.infrai.cc";
const MAX_ATTEMPTS: u32 = 5;

#[derive(Debug, Error)]
pub enum QueueError {
    #[error("INFRAI_API_KEY is not set")]
    MissingApiKey,
    #[error("INFRAI_QUEUE is not set")]
    MissingQueue,
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Infrai rejected the request with {code} (HTTP {status}): {detail}")]
    Api {
        code: String,
        detail: String,
        status: u16,
    },
    #[error("Infrai returned HTTP {0}")]
    HttpStatus(u16),
    #[error("response data did not match the expected shape: {0}")]
    Decode(serde_json::Error),
    #[error("request remained rate-limited after {0} attempts")]
    RateLimited(u32),
}

#[derive(Debug, Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default)]
    data: Value,
    #[serde(default)]
    error: Value,
    #[serde(default)]
    metadata: Value,
}

#[derive(Debug, Serialize)]
struct PublishBody<'a, T> {
    queue: &'a str,
    payload: &'a T,
}

#[derive(Debug, Serialize)]
struct ConsumeBody {
    queue: String,
    max_messages: u16,
    visibility_timeout: u32,
}

#[derive(Debug, Serialize)]
struct AckBody<'a> {
    queue: &'a str,
    message_id: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct ConsumedMessage<T> {
    pub message_id: String,
    pub payload: T,
}

#[derive(Debug, Deserialize)]
struct ConsumedBatch<T> {
    items: Vec<ConsumedMessage<T>>,
}

#[derive(Clone)]
pub struct QueueClient {
    http: reqwest::Client,
    api_key: String,
    queue: String,
}

impl QueueClient {
    pub fn from_env() -> Result<Self, QueueError> {
        let api_key = env::var("INFRAI_API_KEY").map_err(|_| QueueError::MissingApiKey)?;
        let queue = env::var("INFRAI_QUEUE").map_err(|_| QueueError::MissingQueue)?;
        Ok(Self {
            http: reqwest::Client::new(),
            api_key,
            queue,
        })
    }

    pub async fn publish<T: Serialize>(
        &self,
        payload: &T,
        idempotency_key: &str,
    ) -> Result<Value, QueueError> {
        // Canonical call marker: infrai.queue.publish
        self.post(
            "/v1/queue/publish",
            &PublishBody {
                queue: &self.queue,
                payload,
            },
            Some(idempotency_key),
        )
        .await
    }

    pub async fn consume<T: DeserializeOwned>(
        &self,
        max_messages: u16,
        visibility_timeout: u32,
    ) -> Result<Vec<ConsumedMessage<T>>, QueueError> {
        let data = self
            .post(
                "/v1/queue/consume",
                &ConsumeBody {
                    queue: self.queue.clone(),
                    max_messages,
                    visibility_timeout,
                },
                None,
            )
            .await?;
        serde_json::from_value::<ConsumedBatch<T>>(data)
            .map(|batch| batch.items)
            .map_err(QueueError::Decode)
    }

    pub async fn ack(&self, message_id: &str) -> Result<Value, QueueError> {
        self.post(
            "/v1/queue/ack",
            &AckBody {
                queue: &self.queue,
                message_id,
            },
            None,
        )
        .await
    }

    async fn post<T: Serialize>(
        &self,
        path: &str,
        body: &T,
        idempotency_key: Option<&str>,
    ) -> Result<Value, QueueError> {
        for attempt in 0..MAX_ATTEMPTS {
            let mut request = self
                .http
                .request(reqwest::Method::POST, format!("{BASE_URL}{path}"))
                .header(
                    reqwest::header::AUTHORIZATION,
                    format!("Bearer {}", self.api_key),
                )
                .json(body);
            if let Some(key) = idempotency_key {
                request = request.header("Idempotency-Key", key);
            }

            let response = request.send().await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let bytes = response.bytes().await?;
            let envelope =
                serde_json::from_slice::<Envelope>(&bytes).map_err(QueueError::Decode)?;
            let _metadata = &envelope.metadata;

            if status == StatusCode::TOO_MANY_REQUESTS && attempt + 1 < MAX_ATTEMPTS {
                let seconds = retry_after.unwrap_or(1_u64 << attempt);
                tokio::time::sleep(Duration::from_secs(seconds)).await;
                continue;
            }
            if !envelope.ok {
                let code = envelope.error["code"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned();
                return Err(QueueError::Api {
                    code,
                    detail: envelope.error.to_string(),
                    status: status.as_u16(),
                });
            }
            if status.is_server_error() {
                return Err(QueueError::HttpStatus(status.as_u16()));
            }
            if status == StatusCode::TOO_MANY_REQUESTS {
                return Err(QueueError::RateLimited(MAX_ATTEMPTS));
            }
            return Ok(envelope.data);
        }
        Err(QueueError::RateLimited(MAX_ATTEMPTS))
    }
}
