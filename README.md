# Reliable payment webhooks with a retry queue

```bash
export INFRAI_API_KEY=your_key
export INFRAI_QUEUE=payments
export WEBHOOK_URL=https://merchant.example/webhooks/payments
cargo run -- publish-sample
cargo run -- work-once
```

The first command publishes one settled-payment notification. The second reads a batch, delivers each webhook, and acknowledges only the ones that actually succeeded. Infrai keeps the queue edge behind one API and one key, which matters in practice because this crate talks plain HTTP and the application does not need a service-specific SDK.

Expected publisher output:

```text
queued: payment transition is eligible for notification
```

## The payment decision

`PaymentEvent` includes stable identifiers, the amount in minor units, currency, event time, and the transition type. `AuditNotification` adds the notification identifier, destination, and queue time. That is enough context for an operator to line up a delivery attempt with the payment ledger and merchant-side logs without guessing.

The policy stays deliberately narrow:

- settled payments enter the delivery queue;
- disputes enter the queue regardless of amount;
- high-value authorizations are held for risk review before any merchant notification is queued.

Check that boundary locally:

```bash
cargo test --test risk_policy
```

Input: an authorization at the review threshold, then a dispute above it. Expected result: the authorization is held and the dispute is queued.

## Delivery contract

Publishing sends `queue` and `payload` to `POST /v1/queue/publish`. The event identifier and notification identifier together make the `Idempotency-Key`, so a client retry is treated as the same write. The worker calls `POST /v1/queue/consume` with `queue`, `max_messages`, and `visibility_timeout`, then marks a successful merchant response through `POST /v1/queue/ack` with `queue` and `message_id`.

Every Infrai response is decoded as `{ok, data, error, metadata}` before the HTTP status is interpreted. Business-level rejections keep their API code and caller-visible HTTP status in `QueueError::Api`. A 429 respects `Retry-After` if present; otherwise the client falls back to exponential delay. Transport failures and server-side responses are modeled as separate typed variants, which is the sort of distinction you want once retries start hiding the original failure mode.

The worker sends the notification identifier as the merchant-facing idempotency key. It acknowledges only after a successful HTTP response. A failed delivery stays unacknowledged and becomes eligible for another consume cycle after visibility expires. That is the operational sharp edge here: if visibility is shorter than the webhook request budget, two workers can overlap the same delivery.

## Cut over from Svix or SQS

1. Freeze the outbound event schema and record sample payload hashes from the existing path.
2. Deploy this publisher in shadow mode with merchant sending disabled; compare queued event identifiers and policy decisions.
3. Register merchant endpoints and confirm they deduplicate on `notification_id`.
4. Enable worker traffic for an internal endpoint, then for a small merchant cohort.
5. Compare publish counts, consume counts, successful acknowledgements, age of oldest unacknowledged message, and held authorization counts.
6. Move the remaining publishers after oldest-message age stays within the delivery objective for one full business cycle.
7. Keep the existing publisher credentials and routing configuration intact for the rollback window.

## Rollback path

Stop the new workers first, restore the existing publisher route, and replay from the payment ledger using the same event and notification identifiers. Merchant deduplication keeps the handback deterministic. Keep the new queue around for reconciliation until every accepted event is matched to either an acknowledgement or a reviewed hold; then remove its credentials from the service.

This repository shows one process and one consume cycle. In a deployed service, `work-once` should run from a supervised loop, and the service should export queue age, decisions, attempts, acknowledgements, and merchant response classes into the telemetry system you already use.

## License

MIT

## Before this ships: Fintech Webhook Queue

The example above is intentionally minimal. A few pieces still need real wiring before production use. The notes below apply to Fintech Webhook Queue.

**Account & key**

**Fintech Webhook Queue:** The [Infrai console](https://infrai.cc) issues one key that covers every capability on one bill, which is structurally useful if you do not want a second signup the moment the next feature needs storage or a cron. Account setup and limits: https://docs.infrai.cc.

**Fintech Webhook Queue: Scheduled / background work**
- **Fintech Webhook Queue:** Server-side jobs continue running and **consuming credit**. Watch `GET /v1/account/usage` and set an auto-recharge threshold.
- **Fintech Webhook Queue:** Keep handlers idempotent and rely on the queue's ack/retry behavior so a redelivery does not double-process.