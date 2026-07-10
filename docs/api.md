# Backend API

The backend starts an Axum HTTP server using:

- `DATABASE_URL`, required
- `SERVER_HOST`, optional, defaults to `127.0.0.1`
- `SERVER_PORT`, optional, defaults to `3000`

Run:

```bash
cargo run
```

## Health

```http
GET /health
```

Response:

```json
{
  "status": "ok"
}
```

## Collections

```http
GET /collections
POST /collections
GET /collections/{slug}
PUT /collections/{slug}
DELETE /collections/{slug}
```

Create/update body:

```json
{
  "slug": "bukhari",
  "name": "bukhari"
}
```

## Hadiths

```http
GET /hadiths
POST /hadiths
GET /hadiths/{id}
GET /hadiths/by-reference/{collection}/{book_number}/{hadith_number}
PUT /hadiths/{id}
DELETE /hadiths/{id}
```

Supported list filters:

```http
GET /hadiths?collection=bukhari&book_number=1&hadith_number=1&grade=Sahih&limit=50&offset=0
```

Reference lookup:

```http
GET /hadiths/by-reference/bukhari/1/1
```

Create/update body:

```json
{
  "collection_slug": "bukhari",
  "book_number": "1",
  "bab_id": 1.0,
  "english_bab_number": "1",
  "arabic_bab_number": "1",
  "hadith_number": "1",
  "our_hadith_number": 1,
  "arabic_urn": 100010,
  "arabic_bab_name": "باب كَيْفَ كَانَ بَدْءُ الْوَحْىِ",
  "arabic_text": "[prematn]...[/prematn][matn]...[/matn]",
  "arabic_transliteration": null,
  "arabic_grade": "صحيح",
  "english_urn": 10,
  "english_bab_name": "How the Divine Revelation started being revealed",
  "english_text": "Narrated 'Umar bin Al-Khattab: ...",
  "english_grade": "Sahih",
  "last_updated": "2021-03-04 23:36:31",
  "xrefs": ""
}
```

## Retrieval

```http
POST /retrieval
```

Request body:

```json
{
  "query": "intentions",
  "collection": "bukhari",
  "limit": 10
}
```

Current behavior:

```json
{
  "code": "not_implemented",
  "message": "not implemented: retrieval is not implemented yet for query `intentions`"
}
```

The endpoint exists so clients can integrate against the API shape. The actual
retrieval pipeline is still marked with a TODO in the service layer.
