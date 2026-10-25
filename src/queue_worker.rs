use crate::{
    payment_event::{dispatch_decision, DispatchDecision, PaymentEvent},
    queue_client::{QueueClient, QueueError},
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditNotification {
    pub notification_id: String,
    pub destination: String,
    pub event: PaymentEvent,
    pub queued_at: String,
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error(transparent)]
    Queue(#[from] QueueError),
    #[error("webhook transport error: {0}")]
    WebhookTransport(#[from] reqwest::Error),
    #[error("webhook returned HTTP {0}")]
    WebhookStatus(StatusCode),
}

pub async fn enqueue_if_allowed(
    queue: &QueueClient,
    notification: &AuditNotification,
    review_threshold_minor: u64,
) -> Result<DispatchDecision, WorkerError> {
    let decision = dispatch_decision(&notification.event, review_threshold_minor);
    if matches!(decision, DispatchDecision::Enqueue { .. }) {
        let key = format!(
            "payment-event:{}:{}",
            notification.event.event_id, notification.notification_id
        );
        queue.publish(notification, &key).await?;
    }
    Ok(decision)
}

pub async fn drain_once(queue: &QueueClient) -> Result<usize, WorkerError> {
    let messages = queue.consume::<AuditNotification>(10, 45).await?;
    let http = reqwest::Client::new();
    let mut delivered = 0;

    for message in messages {
        let response = http
            .request(reqwest::Method::POST, &message.payload.destination)
            .header("Idempotency-Key", &message.payload.notification_id)
            .json(&message.payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(WorkerError::WebhookStatus(response.status()));
        }
        queue.ack(&message.message_id).await?;
        delivered += 1;
    }

    Ok(delivered)
}
