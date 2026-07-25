ALTER TABLE hadiths
DROP CONSTRAINT hadiths_collection_book_hadith_unique;

-- A published Hadith number may have multiple independently sourced variants.
-- Keep the reference searchable without treating it as a canonical record ID.
CREATE INDEX hadiths_collection_book_hadith_idx
ON hadiths (collection_id, book_number, hadith_number);
