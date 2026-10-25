use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaymentKind {
    Authorized,
    Settled,
    DisputeOpened,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentEvent {
    pub event_id: String,
    pub payment_id: String,
    pub occurred_at: String,
    pub amount_minor: u64,
    pub currency: String,
    pub kind: PaymentKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchDecision {
    Enqueue { reason: &'static str },
    HoldForReview { reason: &'static str },
}

pub fn dispatch_decision(event: &PaymentEvent, review_threshold_minor: u64) -> DispatchDecision {
    match event.kind {
        PaymentKind::DisputeOpened => DispatchDecision::Enqueue {
            reason: "disputes require prompt merchant visibility",
        },
        PaymentKind::Authorized if event.amount_minor >= review_threshold_minor => {
            DispatchDecision::HoldForReview {
                reason: "high-value authorization requires risk review",
            }
        }
        PaymentKind::Authorized | PaymentKind::Settled => DispatchDecision::Enqueue {
            reason: "payment transition is eligible for notification",
        },
    }
}
