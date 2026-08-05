CREATE TABLE narrators (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    hadith_id BIGINT NOT NULL REFERENCES hadiths(id) ON DELETE CASCADE,
    external_id BIGINT,
    role TEXT NOT NULL,
    name TEXT NOT NULL,
    "position" INT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT narrators_name_not_empty CHECK (length(btrim(name)) > 0),
    CONSTRAINT narrators_role_not_empty CHECK (length(btrim(role)) > 0)
);

CREATE INDEX narrators_hadith_id_idx ON narrators (hadith_id);
