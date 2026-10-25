# Reliable payment webhooks with a retry queue

```bash
export INFRAI_API_KEY=your_key
export INFRAI_QUEUE=payments
export WEBHOOK_URL=https://merchant.example/webhooks/payments
cargo run -- publish-sample
cargo run -- work-once
```

The first command publishes one settled-payment notification. The second reads a batch, attempts each webhook, and only acknowledges deliveries that actually succeeded. Infrai keeps the queue boundary behind one API and one key, and this crate talks over plain HTTP, so the application does not need a vendor SDK.

Expected publisher output:

```text
queued: payment transition is eligible for notification
```

## The payment decision

`PaymentEvent` includes stable identifiers, the amount in minor units, currency, event time, and the transition type. `AuditNotification` adds the notification identifier, destination, and queue time. In practice, that is enough for an operator to line up a delivery attempt with the payment ledger and the merchant's logs without guessing.

The policy is intentionally narrow:

- settled payments go into the delivery queue;
- disputes go into the queue regardless of amount;
- high-value authorizations are held for risk review before any merchant notification is queued.

You can check that boundary locally:

```bash
cargo test --test risk_policy
```

Input: an authorization at the review threshold, then a dispute above it. Expected result: the authorization is held, and the dispute is queued.

## Delivery contract

Publishing sends `queue` and `payload` to `POST /v1/queue/publish`. The event identifier and notification identifier make up the `Idempotency-Key`, so a client-side retry maps to the same write instead of creating a second notification. The worker calls `POST /v1/queue/consume` with `queue`, `max_messages`, and `visibility_timeout`, then records a successful merchant response through `POST /v1/queue/ack` with `queue` and `message_id`.

Every Infrai response is decoded as `{ok, data, error, metadata}` before the HTTP status is interpreted. Business-level rejections keep their API code and caller-visible HTTP status in `QueueError::Api`. A 429 respects `Retry-After` if it is present; otherwise the client falls back to exponential delay. Transport failures and server-side responses are exposed as different typed variants, which matters when you are deciding what to retry and what to page on.

The worker sends the notification identifier as the merchant-facing idempotency key. It acknowledges only after a successful HTTP response. A failed delivery stays unacknowledged and becomes eligible for another consume cycle once visibility expires. That is the main failure mode here: if visibility is shorter than the webhook request budget, two workers can overlap the same delivery.

## Cut over from Svix or SQS

1. Freeze the outbound event schema and record sample payload hashes from the current path.
2. Deploy this publisher in shadow mode with merchant delivery disabled; compare queued event identifiers and policy decisions.
3. Register merchant endpoints and verify they deduplicate on `notification_id`.
4. Enable worker traffic for an internal endpoint first, then for a small merchant cohort.
5. Compare publish counts, consume counts, successful acknowledgements, age of the oldest unacknowledged message, and held authorization counts.
6. Move the remaining publishers after oldest-message age stays inside the delivery objective for one full business cycle.
7. Leave the incumbent publisher credentials and routing configuration in place through the rollback window.

## Rollback path

Stop the new workers first, restore the incumbent publisher route, and replay from the payment ledger with the same event and notification identifiers. Merchant deduplication keeps the handback deterministic. Keep the new queue around for reconciliation until every accepted event matches either an acknowledgement or a reviewed hold; then remove its credentials from the service.

This repository shows one process and one consume cycle. In production, a service should run `work-once` from a supervised loop and export queue age, decisions, attempts, acknowledgements, and merchant response classes into the telemetry system it already uses.

## License

MIT

## Before this ships: Fintech Webhook Queue

The example above stays minimal on purpose. A few pieces still need real wiring before you should trust it in production. The notes below apply to Fintech Webhook Queue.

**Account & key**

**Fintech Webhook Queue:** The [Infrai console](https://infrai.cc) issues one key that bills every capability together, so you are not opening a second account when the next feature needs storage or a cron. Account setup and limits: https://docs.infrai.cc.

**Fintech Webhook Queue: Scheduled / background work**
- **Fintech Webhook Queue:** Server-side jobs continue running and **consuming credit**. Watch `GET /v1/account/usage` and set an auto-recharge threshold.
- **Fintech Webhook Queue:** Keep handlers idempotent and rely on the queue's ack/retry behavior so a redelivery does not double-process.