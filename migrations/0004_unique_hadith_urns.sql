-- Migration 0002 deliberately dropped uniqueness on
-- (collection_id, book_number, hadith_number), because one published Hadith
-- number may have several independently sourced variants. That left the table
-- with no stable identity a re-import could match on, so importing the same
-- dump twice silently duplicated canonical records.
--
-- arabicURN and englishURN are the source dump's own stable identifiers and are
-- carried through import unchanged, which makes the pair the right key for
-- skip-if-already-imported. Verified against the live dataset before adding:
-- 44,896 rows, 44,896 distinct pairs, no zero placeholders.
CREATE UNIQUE INDEX hadiths_arabic_english_urn_unique
ON hadiths (arabic_urn, english_urn);
