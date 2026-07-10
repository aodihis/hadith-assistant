# Hadith JSON Import

The backend imports Hadith source data from a local JSON file. Source data files
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

The schema has a nullable `arabic_transliteration` column on `hadiths`.

For now this is intentionally simple. When we add transliteration, the importer
or a later maintenance command can fill that column from `arabic_text` using a
fixed deterministic rule set. There is no separate transliteration scheme table
or version field yet.

## Commands

Validate the JSON without writing to the database:

```bash
cargo run --bin import_hadiths -- data/imports/hadiths.json --validate-only
```

Import into PostgreSQL:

```bash
DATABASE_URL=postgres://user:password@localhost:5432/hadiths \
cargo run --bin import_hadiths -- data/imports/hadiths.json
```

You can also pass the database URL as an argument:

```bash
cargo run --bin import_hadiths -- data/imports/hadiths.json \
  --database-url postgres://user:password@localhost:5432/hadiths
```
