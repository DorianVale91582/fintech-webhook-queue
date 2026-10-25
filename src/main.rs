use fintech_webhook_queue::{
    payment_event::{DispatchDecision, PaymentEvent, PaymentKind},
    queue_client::QueueClient,
    queue_worker::{drain_once, enqueue_if_allowed, AuditNotification},
};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command = env::args()
        .nth(1)
        .unwrap_or_else(|| "publish-sample".into());
    let queue = QueueClient::from_env()?;

    match command.as_str() {
        "publish-sample" => {
            let destination = env::var("WEBHOOK_URL")?;
            let notification = AuditNotification {
                notification_id: "notice_evt_2026_09_08_001".into(),
                destination,
                queued_at: "2026-09-08T09:30:01Z".into(),
                event: PaymentEvent {
                    event_id: "evt_2026_09_08_001".into(),
                    payment_id: "pay_1842".into(),
                    occurred_at: "2026-09-08T09:30:00Z".into(),
                    amount_minor: 12_500,
                    currency: "USD".into(),
                    kind: PaymentKind::Settled,
                },
            };
            match enqueue_if_allowed(&queue, &notification, 1_000_000).await? {
                DispatchDecision::Enqueue { reason } => println!("queued: {reason}"),
                DispatchDecision::HoldForReview { reason } => println!("held: {reason}"),
            }
        }
        "work-once" => println!("delivered: {}", drain_once(&queue).await?),
        other => return Err(format!("unknown command: {other}").into()),
    }
    Ok(())
}
