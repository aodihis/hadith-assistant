CREATE TABLE collections (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT collections_slug_not_empty CHECK (length(btrim(slug)) > 0),
    CONSTRAINT collections_name_not_empty CHECK (length(btrim(name)) > 0)
);

CREATE TABLE hadiths (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    collection_id BIGINT NOT NULL REFERENCES collections(id),
    book_number TEXT NOT NULL,
    bab_id DOUBLE PRECISION NOT NULL,
    english_bab_number TEXT,
    arabic_bab_number TEXT,
    hadith_number TEXT NOT NULL,
    our_hadith_number INTEGER NOT NULL,
    arabic_urn BIGINT NOT NULL UNIQUE,
    arabic_bab_name TEXT,
    arabic_text TEXT NOT NULL,
    arabic_transliteration TEXT,
    arabic_grade TEXT NOT NULL DEFAULT '',
    english_urn BIGINT NOT NULL UNIQUE,
    english_bab_name TEXT,
    english_text TEXT,
    english_grade TEXT NOT NULL DEFAULT '',
    last_updated TEXT,
    xrefs TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT hadiths_book_number_not_empty CHECK (length(btrim(book_number)) > 0),
    CONSTRAINT hadiths_hadith_number_not_empty CHECK (length(btrim(hadith_number)) > 0),
    CONSTRAINT hadiths_arabic_text_not_empty CHECK (length(btrim(arabic_text)) > 0),
    CONSTRAINT hadiths_arabic_urn_positive CHECK (arabic_urn > 0),
    CONSTRAINT hadiths_english_urn_positive CHECK (english_urn > 0)
);

CREATE INDEX hadiths_collection_book_idx
    ON hadiths (collection_id, book_number);

CREATE INDEX hadiths_collection_hadith_number_idx
    ON hadiths (collection_id, hadith_number);

CREATE INDEX hadiths_arabic_grade_idx
    ON hadiths (arabic_grade);

CREATE INDEX hadiths_english_grade_idx
    ON hadiths (english_grade);
