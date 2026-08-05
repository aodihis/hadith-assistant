use sqlx::PgPool;

use crate::ingestion::narrator::{insert_narrators, parse_isnad};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BackfillSummary {
    pub rows_scanned: usize,
    pub rows_updated: usize,
    pub rows_skipped_already_processed: usize,
    pub rows_failed: usize,
}

#[derive(Debug, sqlx::FromRow)]
struct BackfillRow {
    id: i64,
    arabic_text: String,
    english_text: Option<String>,
}

/// Re-parses every hadith's `arabic_text` for isnad markup, cleaning the
/// stored text and populating `narrators`. Safe to re-run: rows that
/// already have narrators are excluded by the batch query itself (a single
/// `NOT EXISTS` per batch rather than a round trip per row), and each row
/// commits independently so one bad row doesn't block the rest of the
/// table.
pub async fn backfill_narrators(pool: &PgPool) -> Result<BackfillSummary, sqlx::Error> {
    const BATCH_SIZE: i64 = 500;

    let rows_scanned: i64 = sqlx::query_scalar("SELECT count(*) FROM hadiths")
        .fetch_one(pool)
        .await?;

    let mut summary = BackfillSummary {
        rows_scanned: rows_scanned as usize,
        ..Default::default()
    };
    let mut last_id = 0i64;

    loop {
        let rows = sqlx::query_as::<_, BackfillRow>(
            r#"
            SELECT h.id, h.arabic_text, h.english_text
            FROM hadiths h
            WHERE h.id > $1
              AND NOT EXISTS (SELECT 1 FROM narrators n WHERE n.hadith_id = h.id)
            ORDER BY h.id
            LIMIT $2
            "#,
        )
        .bind(last_id)
        .bind(BATCH_SIZE)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            break;
        }
        last_id = rows.last().expect("batch checked non-empty above").id;

        for row in rows {
            match backfill_row(pool, &row).await {
                Ok(()) => summary.rows_updated += 1,
                Err(error) => {
                    tracing::warn!(hadith_id = row.id, %error, "narrator backfill failed for row");
                    summary.rows_failed += 1;
                }
            }
        }
    }

    summary.rows_skipped_already_processed = summary
        .rows_scanned
        .saturating_sub(summary.rows_updated)
        .saturating_sub(summary.rows_failed);

    Ok(summary)
}

async fn backfill_row(pool: &PgPool, row: &BackfillRow) -> Result<(), sqlx::Error> {
    let parsed = parse_isnad(&row.arabic_text, row.english_text.as_deref());

    let mut tx = pool.begin().await?;

    if parsed.clean_arabic_text != row.arabic_text {
        sqlx::query("UPDATE hadiths SET arabic_text = $1, updated_at = now() WHERE id = $2")
            .bind(&parsed.clean_arabic_text)
            .bind(row.id)
            .execute(&mut *tx)
            .await?;
    }

    insert_narrators(&mut tx, row.id, &parsed.narrators).await?;

    tx.commit().await?;

    Ok(())
}
