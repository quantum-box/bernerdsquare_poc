# Prototype API

All API routes require `Authorization: Bearer <token>`. The Worker reads `API_BEARER_TOKENS_JSON` as a secret JSON object mapping opaque tokens to server-side member IDs. The client never submits an owner ID. Requests are scoped to that mapped owner. Invalid credentials receive `401`; a resource owned by another member is indistinguishable from a missing resource.

The API currently uses mock adapters only. It persists state in D1 and returns `mode: "mock"`. `gate_applied` and `physical_unlock_confirmed` are always false.

## Routes

| Method | Path | Purpose |
|---|---|---|
| GET | `/healthz` | Health and non-secret mock/auth configuration state |
| POST | `/v1/credentials/issue` | Create a mock credential reference; no NFC payload is created |
| GET | `/v1/credentials/{id}` | Read an owner-scoped credential |
| POST | `/v1/reservations` | Create a test reservation; interval is `[starts_at, ends_at)` |
| GET | `/v1/reservations/{id}` | Read an owner-scoped reservation |
| PATCH | `/v1/reservations/{id}` | Change the complete test reservation interval |
| DELETE | `/v1/reservations/{id}` | Cancel the reservation and revoke its mock registrations |
| POST | `/v1/registrations` | Request a mock registration for an owned credential and reservation |
| GET | `/v1/registrations/{id}` | Read registration state and simulated adapter result |
| DELETE | `/v1/registrations/{id}` | Revoke the mock registration |
| POST | `/v1/registrations/{id}/retry` | Retry a failed mock registration |
| POST | `/v1/authorizations/check` | Simulate whether an active registration is within its reservation window |
| GET | `/v1/events?session_id={id}` | Read sanitized owner-scoped audit events |

Mutation bodies carry `request_id` for idempotency and may carry a `session_id` for correlation. The app also sends `X-Session-ID`; `DELETE` routes use that value for event correlation and an `Idempotency-Key` header for replay protection. A repeated key with the same request returns the stored response. Reusing it with a different body returns `409`.

## Time checks

All times must be RFC 3339 with an explicit offset. The API normalizes stored values to UTC. Authorization is allowed only when the reservation is active, the mock registration is active, and `starts_at <= now < ends_at`. Supplying `evaluated_at` is available only when `ALLOW_TIME_SIMULATION=true`, which is intended for local test runs. It never simulates a physical reader or unlock.

For a local failure/retry check, create a registration with `simulate_failure: true` while `ALLOW_TIME_SIMULATION=true`, then call `/v1/registrations/{id}/retry` with a new `request_id`. The switch is rejected when the Worker variable is false. The iOS mock screen exposes boundary checks; API mode checks current server time, because custom `evaluated_at` is accepted only by a Worker configured for local time simulation. Repeating a registration request with the same request ID is idempotent; another request ID for the same active or failed credential-reservation pair returns `409` instead of creating a duplicate.

## Local development

1. Install the Rust `wasm32-unknown-unknown` target, `worker-build`, and Wrangler.
2. Create `api/.dev.vars` from `.dev.vars.example` and use a fresh local-only token.
3. From `api/`, run the D1 migration locally, then start `wrangler dev`.
4. For deployment, provision the D1 database through the approved CloudApp path, replace the placeholder database ID in `wrangler.toml`, apply the migration, and set `API_BEARER_TOKENS_JSON` as a server-side secret.

No production database, secret, CloudApp app, or deployment has been configured from this repository.
