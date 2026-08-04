# Hadith JSON Import

The application imports Hadith source data from a local JSON file. Source data files
belong in `data/imports/`, which is ignored by Git because the dataset comes
from an external source.

## Expected Shape

The importer expects a top-level object with a `HadithTable` array. Each item is
one Hadith row.

```json
{
  "HadithTable": [
    {
      "collection": "bukhari",
      "bookNumber": "1",
      "babID": 1.0,
      "englishBabNumber": "1",
      "arabicBabNumber": "1",
      "hadithNumber": "1",
      "ourHadithNumber": 1,
      "arabicURN": 100010,
      "arabicBabName": "باب كَيْفَ كَانَ بَدْءُ الْوَحْىِ",
      "arabicText": "[prematn]...[/prematn][matn]...[/matn]",
      "arabicgrade1": "صحيح",
      "englishURN": 10,
      "englishBabName": "How the Divine Revelation started being revealed",
      "englishText": "Narrated 'Umar bin Al-Khattab: ...",
      "englishgrade1": "Sahih",
      "last_updated": "2021-03-04 23:36:31",
      "xrefs": ""
    }
  ]
}
```

## Tables

The import writes to two tables:

- `collections`: one row per collection slug, such as `bukhari`, `muslim`, or
  `mishkat`
- `hadiths`: one row per JSON Hadith record

The `hadiths` table follows the JSON closely. Grades live on the Hadith row as
`arabic_grade` and `english_grade`, matching the source fields `arabicgrade1`
and `englishgrade1`.

The importer also derives `arabic_transliteration` from `arabicText` by calling
the local deterministic transliteration library.

## Required Fields

The importer requires:

- `collection`
- `bookNumber`
- `hadithNumber` or `ourHadithNumber`, so a non-empty Hadith number can be
  stored
- `arabicURN`, greater than `0`
- `englishURN`, greater than `0`
- `arabicText`, non-empty

`hadithNumber` is preferred. If it is blank, the importer uses
`ourHadithNumber` when it is greater than `0`.

## Transliteration

The importer fills `hadiths.arabic_transliteration` during import. It calls
`simple-readable-v1`, which is a local deterministic function that accepts
Arabic text and returns Latin transliteration.

The original Arabic source text remains authoritative and is never modified.

## Commands

Validate the JSON without writing to the database:

```bash
cargo run --bin import_hadiths -- data/imports/hadiths.json --validate-only
```

Import into PostgreSQL:

```bash
docker compose up -d postgres
cargo run --bin import_hadiths -- data/imports/hadiths.json
```

The import CLI loads `DATABASE_URL` from `.env` automatically. You can still
override it with an explicit argument:

You can also pass the database URL as an argument:

```bash
cargo run --bin import_hadiths -- data/imports/hadiths.json \
  --database-url postgres://user:password@localhost:5432/hadiths
```

## Embedding for retrieval

Add `--embed` to also index the newly imported records into Qdrant for
`POST /api/retrieval`:

```bash
docker compose up -d postgres qdrant
cargo run --bin import_hadiths -- data/imports/hadiths.json --embed
```

`--embed` only embeds the records inserted by that import run, not the whole
table — it re-fetches them by the IDs the import just created, embeds their
combined Arabic and English text through the configured embedding provider
(`EMBEDDING_BASE_URL`, `EMBEDDING_API_KEY`, `EMBEDDING_MODEL` in the root
[README](../README.md)), and upserts the resulting vectors into Qdrant
(`QDRANT_URL`, `QDRANT_COLLECTION`), creating the collection on first use if
it does not already exist. `EMBEDDING_API_KEY` and a reachable Qdrant
instance are required when `--embed` is passed; plain `import_hadiths`
without the flag needs neither.
