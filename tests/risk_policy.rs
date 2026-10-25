use fintech_webhook_queue::payment_event::{
    dispatch_decision, DispatchDecision, PaymentEvent, PaymentKind,
};

fn event(kind: PaymentKind, amount_minor: u64) -> PaymentEvent {
    PaymentEvent {
        event_id: "evt_test_1".into(),
        payment_id: "pay_test_1".into(),
        occurred_at: "2026-09-08T09:30:00Z".into(),
        amount_minor,
        currency: "USD".into(),
        kind,
    }
}

#[test]
fn holds_high_value_authorization_but_dispatches_dispute() {
    let threshold = 1_000_000;
    assert!(matches!(
        dispatch_decision(&event(PaymentKind::Authorized, threshold), threshold),
        DispatchDecision::HoldForReview { .. }
    ));
    assert!(matches!(
        dispatch_decision(&event(PaymentKind::DisputeOpened, threshold * 2), threshold),
        DispatchDecision::Enqueue { .. }
    ));
}
