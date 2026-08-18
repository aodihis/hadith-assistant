-- `collections.name` has always duplicated `slug`, because the importer derives
-- it from the source dump's collection key. That surfaces raw slugs like
-- `riyadussalihin` in the browser's filter dropdown and in the JSON API.
--
-- Collection names are source metadata, so these are the established titles of
-- each work rather than anything invented. Collections whose identity is not
-- certain from the data are deliberately left untouched: showing the raw slug
-- is honest, whereas guessing at the title of a religious work is not.
--
-- The `name = slug` guard means this only fills in names that were never set,
-- so a curated name is never overwritten and re-running is harmless.
UPDATE collections SET name = v.name
FROM (VALUES
    ('bukhari',         'Sahih al-Bukhari'),
    ('muslim',          'Sahih Muslim'),
    ('nasai',           'Sunan an-Nasa''i'),
    ('abudawud',        'Sunan Abi Dawud'),
    ('tirmidhi',        'Jami'' at-Tirmidhi'),
    ('ibnmajah',        'Sunan Ibn Majah'),
    ('ahmad',           'Musnad Ahmad'),
    ('adab',            'Al-Adab Al-Mufrad'),
    ('shamail',         'Ash-Shama''il Al-Muhammadiyah'),
    ('bulugh',          'Bulugh al-Maram'),
    ('mishkat',         'Mishkat al-Masabih'),
    ('riyadussalihin',  'Riyad as-Salihin'),
    ('hisn',            'Hisn al-Muslim'),
    ('virtues',         'Virtues of the Qur''an')
) AS v(slug, name)
WHERE collections.slug = v.slug
  AND collections.name = collections.slug;
