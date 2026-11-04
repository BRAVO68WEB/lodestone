use color_eyre::eyre::Context;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use crate::error::Error;
use crate::events::Event;

pub const CONSOLE_PAGE_DEFAULT: i64 = 300;
const CONSOLE_PAGE_MAX: i64 = 300;

pub struct ConsoleHistoryPage {
    pub lines: Vec<String>,
    pub exhausted: bool,
}

pub fn clamp_console_limit(limit: Option<i64>) -> i64 {
    match limit {
        Some(value) if value < 1 => 1,
        Some(value) if value > CONSOLE_PAGE_MAX => CONSOLE_PAGE_MAX,
        Some(value) => value,
        None => CONSOLE_PAGE_DEFAULT,
    }
}

pub async fn init_console_events(pool: &SqlitePool) -> Result<(), Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS ConsoleEvents (
            instance_id TEXT NOT NULL,
            snowflake INTEGER NOT NULL,
            event_json TEXT NOT NULL,
            PRIMARY KEY (instance_id, snowflake)
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create console history")?;
    Ok(())
}

pub async fn record_console_event(pool: &SqlitePool, event: &Event) -> Result<(), Error> {
    if !event.is_event_console_message() {
        return Ok(());
    }
    let Some(uuid) = event.get_instance_uuid() else {
        return Ok(());
    };
    let json = serde_json::to_string(event).context("Failed to encode a console line")?;
    sqlx::query(
        r#"
        INSERT INTO ConsoleEvents (instance_id, snowflake, event_json)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(instance_id, snowflake) DO NOTHING
        "#,
    )
    .bind(uuid.to_string())
    .bind(event.snowflake.raw())
    .bind(json)
    .execute(pool)
    .await
    .context("Failed to store a console line")?;
    Ok(())
}

pub async fn console_page(
    pool: &SqlitePool,
    instance_id: &str,
    before: i64,
    limit: i64,
) -> Result<ConsoleHistoryPage, Error> {
    let limit = clamp_console_limit(Some(limit));
    let rows = sqlx::query(
        r#"
        SELECT event_json FROM ConsoleEvents
        WHERE instance_id = ?1 AND snowflake < ?2
        ORDER BY snowflake DESC
        LIMIT ?3
        "#,
    )
    .bind(instance_id)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to read console history")?;
    let mut lines: Vec<String> = rows
        .iter()
        .map(|row| row.get::<String, _>("event_json"))
        .collect();
    let exhausted = (lines.len() as i64) < limit;
    lines.reverse();
    Ok(ConsoleHistoryPage { lines, exhausted })
}

pub async fn clear_console_events(pool: &SqlitePool, instance_id: &str) -> Result<(), Error> {
    sqlx::query(r#"DELETE FROM ConsoleEvents WHERE instance_id = ?1"#)
        .bind(instance_id)
        .execute(pool)
        .await
        .context("Failed to clear console history")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sqlx::sqlite::SqliteConnectOptions;
    use sqlx::SqlitePool;

    use crate::events::{CausedBy, Event, EventInner, InstanceEvent, InstanceEventInner};
    use crate::types::{InstanceUuid, Snowflake};

    use super::{
        clamp_console_limit, clear_console_events, console_page, init_console_events,
        record_console_event,
    };

    fn output(instance: &str, snowflake: i64, message: &str) -> Event {
        Event {
            details: String::new(),
            snowflake: Snowflake::from_raw(snowflake),
            event_inner: EventInner::InstanceEvent(InstanceEvent {
                instance_uuid: InstanceUuid::from(instance.to_string()),
                instance_name: "test".to_string(),
                instance_event_inner: InstanceEventInner::InstanceOutput {
                    message: message.to_string(),
                },
            }),
            caused_by: CausedBy::System,
        }
    }

    async fn pool() -> SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("console.db");
        let pool = SqlitePool::connect_with(
            SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
                .unwrap()
                .create_if_missing(true),
        )
        .await
        .unwrap();
        init_console_events(&pool).await.unwrap();
        // Keep the temp dir alive for the pool by leaking it. The test process is short.
        std::mem::forget(dir);
        pool
    }

    #[test]
    fn console_page_limit_stays_between_one_and_300() {
        assert_eq!(clamp_console_limit(None), 300);
        assert_eq!(clamp_console_limit(Some(0)), 1);
        assert_eq!(clamp_console_limit(Some(-4)), 1);
        assert_eq!(clamp_console_limit(Some(50)), 50);
        assert_eq!(clamp_console_limit(Some(10_000)), 300);
    }

    #[tokio::test]
    async fn older_lines_page_before_a_snowflake_and_stay_in_one_instance() {
        let pool = pool().await;
        for (instance, snowflake) in [("alpha", 1), ("alpha", 2), ("alpha", 3), ("beta", 2)] {
            record_console_event(&pool, &output(instance, snowflake, &format!("{instance}-{snowflake}")))
                .await
                .unwrap();
        }
        let state = Event {
            details: String::new(),
            snowflake: Snowflake::from_raw(9),
            event_inner: EventInner::InstanceEvent(InstanceEvent {
                instance_uuid: InstanceUuid::from("alpha".to_string()),
                instance_name: "test".to_string(),
                instance_event_inner: InstanceEventInner::StateTransition {
                    to: crate::traits::t_server::State::Running,
                },
            }),
            caused_by: CausedBy::System,
        };
        record_console_event(&pool, &state).await.unwrap();

        let all = console_page(&pool, "alpha", i64::MAX, 10).await.unwrap();
        assert_eq!(all.lines.len(), 3);

        let page = console_page(&pool, "alpha", 3, 2).await.unwrap();
        assert_eq!(page.lines.len(), 2);
        assert!(page.lines[0].contains("alpha-1"), "{}", page.lines[0]);
        assert!(page.lines[1].contains("alpha-2"), "{}", page.lines[1]);
        assert!(!page.exhausted);

        let none = console_page(&pool, "alpha", 1, 2).await.unwrap();
        assert!(none.lines.is_empty());
        assert!(none.exhausted);

        clear_console_events(&pool, "alpha").await.unwrap();
        let cleared = console_page(&pool, "alpha", i64::MAX, 10).await.unwrap();
        assert!(cleared.lines.is_empty());
        let other = console_page(&pool, "beta", i64::MAX, 10).await.unwrap();
        assert_eq!(other.lines.len(), 1);
        assert!(other.lines[0].contains("beta-2"));
    }
}
