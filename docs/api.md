# Web and JSON API

Topcoat serves the web interface and JSON API from the same Rust binary.
`DATABASE_URL` is required; the rest of the configuration is documented in the
root [README](../README.md).

Pending SQLx migrations run before the server begins accepting requests.

## Local server

Start dependencies and the Topcoat development server:

```bash
docker compose up -d postgres qdrant
topcoat dev
```

The application listens on <http://127.0.0.1:3000> by default. Set `HOST` and
`PORT` to override the bind address.

## Browser pages

```http
GET /
GET /hadiths
GET /chat
```

`/hadiths` accepts the same filters as the JSON list endpoint and renders
canonical records on the server.

`/chat` is the Sanad chat surface. It renders only static chrome; every
narration it shows arrives from `POST /api/chat`, so the page and the JSON API
read through the same retrieval path.

## Error envelope

Application errors use a stable JSON shape:

```json
{
  "code": "validation_error",
  "message": "validation failed: limit must be between 1 and 200"
}
```

Database details and internal stack traces are never returned to clients.

| `code` | Status | Meaning |
| --- | --- | --- |
| `validation_error` | 400 | Malformed or out-of-range input |
| `not_found` | 404 | No such record |
| `conflict` | 409 | Record already exists |
| `session_expired` | 401 | Chat session unknown or past its lifetime |
| `too_many_requests` | 429 | Session request budget exhausted |
| `not_implemented` | 501 | Stage not configured, e.g. no chat API key |
| `database_error`, `internal_error` | 500 | Server-side failure |

## Health

```http
GET /api/health
```

```json
{
  "status": "ok"
}
```

## Collections

```http
GET /api/collections
GET /api/collections/{slug}
```

## Hadiths

```http
GET /api/hadiths
GET /api/hadiths/{id}
GET /api/hadiths/by-reference/{collection}/{book_number}/{hadith_number}
```

Supported list filters:

```http
GET /api/hadiths?collection=bukhari&book_number=1&hadith_number=1&grade=Sahih&limit=50&offset=0
```

- The default `limit` is `50`.
- The maximum `limit` is `200`.
- `offset` must be zero or greater.

Reference lookup:

```http
GET /api/hadiths/by-reference/bukhari/1/1
```

Reference lookup returns an array because some collections assign the same
published Hadith number to multiple independently sourced records or variants.
The combination of collection, book number, and Hadith number is searchable but
is not a canonical record identifier. Use each result's `id`, `arabic_urn`, or
`english_urn` for record-level traceability.

Canonical data is imported through the CLI, so the HTTP API remains read-only.

## Retrieval

```http
POST /api/retrieval
Content-Type: application/json
```

```json
{
  "query": "intentions",
  "collection": "bukhari",
  "limit": 10
}
```

`collection` and `limit` are optional. `limit` defaults to `10` and is capped
at `20`.

Response:

```json
{
  "query": "intentions",
  "results": [
    {
      "hadith_id": 1,
      "collection": "bukhari",
      "book_number": "1",
      "hadith_number": "1",
      "arabic_text": "...",
      "english_text": "...",
      "arabic_grade": "صحيح",
      "english_grade": "Sahih",
      "narrator": { "name": "Umar ibn al-Khattab", "role": "sahabi" },
      "score": 0.83
    }
  ]
}
```

Grades are carried verbatim from the canonical record and are never inferred or
normalized. `narrator` is the primary narrator where one is recorded, and
`null` otherwise.

Matches scoring below `RETRIEVAL_MIN_SCORE` are discarded before the response
is built, so a narration that merely shares vocabulary with the query is not
returned. Measured with `text-embedding-3-small`, natural-language questions
score roughly 0.40-0.55, so a threshold at or above 0.7 discards everything.

Retrieval embeds the query text through the configured embedding provider,
searches Qdrant for the nearest indexed Hadith vectors (optionally scoped to
one collection), and resolves every match back to its canonical PostgreSQL
record before returning it. A vector match that no longer resolves to a
canonical record (e.g. a stale point after a Hadith was removed) is dropped
from the response rather than fabricated; it is logged as a warning.

Hadiths are indexed into Qdrant via `import_hadiths --embed`, or for records
imported earlier `import_hadiths --embed-collection <slug>`, documented in
[docs/import-hadith-json.md](import-hadith-json.md). Source markup is stripped
from the text a vector is built from, while the stored record keeps its
original content.

## Answers

```http
POST /api/answers
Content-Type: application/json
```

```json
{
  "query": "What did the Prophet say about intentions?",
  "collection": "bukhari",
  "limit": 5
}
```

The request body matches `/api/retrieval`: `collection` and `limit` are
optional, `limit` defaults to `10` and is capped at `20`.

Response:

```json
{
  "query": "What did the Prophet say about intentions?",
  "answer": {
    "title": "Intention Behind Actions",
    "answer": "These narrations report that deeds are judged by their intentions..."
  },
  "citations": [
    {
      "hadith_id": 1,
      "collection": "bukhari",
      "book_number": "1",
      "hadith_number": "1",
      "arabic_text": "...",
      "english_text": "...",
      "arabic_grade": "صحيح",
      "english_grade": "Sahih",
      "narrator": { "name": "Umar ibn al-Khattab", "role": "sahabi" },
      "score": 0.83
    }
  ]
}
```

This endpoint runs retrieval first, then generates an answer constrained to
the retrieved records. `citations` is always the full set of canonical records
the answer was generated from, so generated text is never returned without its
sources.

`answer` is `null` — with `citations` still populated — whenever generation is
unavailable rather than successful:

- `OPEN_ROUTER_API_KEY` is unset, so no chat provider is configured.
- Retrieval matched nothing, so there is nothing to ground an answer in. The
  provider is not called at all in this case.
- The provider request failed, or returned output that did not match the
  expected shape.

A `null` answer is a successful `200` response, not an error. The endpoint
never substitutes an ungrounded or fabricated answer for a missing one.

## Chat sessions

```http
POST /api/chat/session
```

```json
{ "token": "1755...b9c.4f2a...", "expires_in_seconds": 43200 }
```

Issues the token `POST /api/chat` requires, sent back in the `x-sanad-session`
header. The token is HMAC-signed and carries its own issue time, so expiry is
verified without any server-side lookup.

It holds **no user identity** — it exists to key rate limiting and to make the
chat endpoint awkward to drive from outside our own pages. It is deliberately
**not authentication**: anyone may request one. It raises the cost of abuse; it
does not prevent a determined caller.

Two budgets apply per session: a short burst window and a lifetime cap.
Exceeding either returns `too_many_requests`.

## Chat

```http
POST /api/chat
Content-Type: application/json
x-sanad-session: <token from /api/chat/session>
```

```json
{
  "message": "What is said about the call to prayer?",
  "collection": null,
  "history": {
    "summary": null,
    "summarized_turns": 0,
    "turns": [{ "question": "…", "answer": "…", "refused": false }]
  }
}
```

`history` is optional; omit it on the first turn. It is **never stored on the
server** — the client holds the conversation and replays it, and the server
hands back a compacted copy each turn.

The response is a `text/event-stream`. Event order is part of the contract:

| Event | Payload | Notes |
| --- | --- | --- |
| `title` | `{ "title": "…" }` | The turn is an answer |
| `citations` | `{ "citations": [ … ] }` | Released only after `title` |
| `delta` | `{ "text": "…" }` | Repeated; answer prose |
| `refusal` | `{ "reason": "off_topic" \| "not_covered", "message": "…" }` | Instead of the three above |
| `memory` | `{ "history": { … }, "compacted": bool }` | Authoritative next-turn history |
| `error` | `{ "code": "…", "message": "…" }` | Generation or validation failed |
| `done` | `{}` | Terminal |

Two properties matter to any client:

**Citations are withheld until the first line proves the turn is an answer.** A
refusal carries none, ever — attaching narrations to a reply that is not about
them would be misleading. Sending citations as soon as retrieval finished would
make them flash on screen and vanish for an off-topic question.

**Commit a turn to local history only when `memory` arrives**, never when
rendering finishes. If the stream drops mid-answer the user keeps their partial
text, but history stays untouched, so the next question replays correctly. A
client that appends optimistically will silently desynchronise the model's
context from the visible conversation.

Compaction is server-side. Once the history passes its turn or size budget, the
oldest turns are folded into `summary` and `compacted` is `true`. The summary
carries hadith references as identifiers only and never narration text: it is
derived notes, not a source. A failed summarisation never fails the turn — it
falls back to dropping the oldest turns and reports `compacted: false`.

## Related narrations

```http
GET /api/hadiths/{id}/related?limit=3
```

```json
{ "hadith_id": 412, "related": [ { …RetrievedHadith… } ] }
```

Finds narrations similar to the given one by embedding its Arabic text. `limit`
defaults to `3` and is capped at `10`. The relevance threshold applied to
query-driven retrieval does **not** apply here, because comparing a narration
against its own text scores on a different scale.

## Migration from the backend-only layout

The full-stack migration moved the former endpoints under `/api`:

| Previous | Current |
| --- | --- |
| `/health` | `/api/health` |
| `/collections` | `/api/collections` |
| `/hadiths` | `/api/hadiths` |
| `/retrieval` | `/api/retrieval` |

The `/hadiths` path is now the server-rendered browser page.
