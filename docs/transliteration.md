# Arabic Transliteration

The backend includes a deterministic local transliterator named
`simple-readable-v1`.

It does not call a third-party service. It maps Arabic characters to readable
Latin text with fixed rules and preserves source markup tags such as
`[narrator ...]` and `[/narrator]` unchanged.

## Library Function

Use the pure Rust function when Arabic text needs transliteration:

```rust
use hadith_assistant::transliteration::simple::transliterate;

let value = transliterate("إِنَّمَا");
assert_eq!(value, "'innamaa");
```

During JSON import, the importer calls this function for each
`arabicText` value and stores the result in `hadiths.arabic_transliteration`.

## Current Scope

This is a simple readable transliteration, not a scholarly standard such as
ISO-233. The original Arabic source text remains authoritative and is never
modified by this process.
