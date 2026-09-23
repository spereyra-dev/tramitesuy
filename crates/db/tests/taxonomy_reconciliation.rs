//! F11 (WU-4): the taxonomy seed reconciles the projection with the YAML,
//! not only upserts it. A relation dropped from an event, an event whose
//! relation list is emptied, and an event/category removed from the YAML all
//! disappear from the database and from the read paths (`categories`,
//! `events_by_category`) — transactionally and idempotently per slug.

mod common;

use common::fresh_migrated_db;
use db::repos::taxonomy_seed::{categories, events_by_category, seed_taxonomy};
use sqlx::PgPool;
use sqlx::types::Uuid;
use taxonomy::model::{
    Category, CategorySource, Event, EventSource, Keyword, KeywordType, Relation, Taxonomy,
};

fn category(slug: &str, name: &str, order: u32) -> CategorySource {
    CategorySource {
        file: format!("categories/{slug}.yaml"),
        category: Category {
            slug: slug.to_string(),
            name: name.to_string(),
            icon: None,
            order_index: order,
        },
    }
}

fn event(slug: &str, category_slug: &str, relations: Vec<Relation>) -> EventSource {
    EventSource {
        file: format!("events/{slug}.yaml"),
        event: Event {
            slug: slug.to_string(),
            name: slug.to_string(),
            description: format!("{slug} description"),
            category: category_slug.to_string(),
            keywords: vec![Keyword {
                term: slug.to_string(),
                keyword_type: KeywordType::Action,
                weight: 5,
                canonical: String::new(),
                negative: false,
            }],
            rules: Vec::new(),
            relations,
            tests: Default::default(),
        },
    }
}

fn relation(external_id: &str, order: u32) -> Relation {
    Relation {
        external_id: external_id.to_string(),
        order,
        required: true,
    }
}

fn taxonomy(categories: Vec<CategorySource>, events: Vec<EventSource>) -> Taxonomy {
    Taxonomy {
        events,
        categories,
        synonyms: Vec::new(),
    }
}

/// One organization + one active procedure per external id (the FK targets
/// the relations resolve against).
async fn seed_procedures(pool: &PgPool, external_ids: &[&str]) {
    let organization_id: Uuid = sqlx::query_scalar(
        "INSERT INTO organizations (external_id, name) VALUES ('recon-org', 'Recon') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed organization");
    for external_id in external_ids {
        sqlx::query(
            "INSERT INTO procedures (external_id, name, organization_id, status) \
             VALUES ($1, 'Trámite', $2, 'active')",
        )
        .bind(external_id)
        .bind(organization_id)
        .execute(pool)
        .await
        .expect("seed procedure");
    }
}

async fn relation_external_ids(pool: &PgPool, event_slug: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT p.external_id FROM life_event_procedures r \
         JOIN life_events e ON e.id = r.life_event_id \
         JOIN procedures p ON p.id = r.procedure_id \
         WHERE e.slug = $1 ORDER BY r.order_index",
    )
    .bind(event_slug)
    .fetch_all(pool)
    .await
    .expect("relations readable")
}

async fn event_exists(pool: &PgPool, slug: &str) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM life_events WHERE slug = $1")
        .bind(slug)
        .fetch_one(pool)
        .await
        .expect("event count")
        > 0
}

async fn category_slugs(pool: &PgPool) -> Vec<String> {
    categories(pool)
        .await
        .expect("categories readable")
        .into_iter()
        .map(|row| row.slug)
        .collect()
}

/// (a) A relation dropped from an event's YAML relation list is deleted from
/// the projection, while the retained relation keeps its order.
#[tokio::test(flavor = "multi_thread")]
async fn a_relation_dropped_from_yaml_is_deleted() {
    let (pool, name) = fresh_migrated_db().await;
    seed_procedures(&pool, &["100", "200"]).await;

    let full = taxonomy(
        vec![category("cat", "Categoría", 1)],
        vec![event(
            "ev",
            "cat",
            vec![relation("100", 1), relation("200", 2)],
        )],
    );
    seed_taxonomy(&pool, &full).await.expect("first seed");
    assert_eq!(
        relation_external_ids(&pool, "ev").await,
        vec!["100", "200"],
        "both YAML relations are projected"
    );

    let reduced = taxonomy(
        vec![category("cat", "Categoría", 1)],
        vec![event("ev", "cat", vec![relation("100", 1)])],
    );
    let report = seed_taxonomy(&pool, &reduced).await.expect("second seed");

    assert_eq!(
        relation_external_ids(&pool, "ev").await,
        vec!["100"],
        "the relation the YAML dropped must be deleted from the projection"
    );
    assert_eq!(
        report.relations_removed, 1,
        "the removed relation is accounted for in the report"
    );

    common::drop_test_db(&name).await;
}

/// (b) Emptying an event's relation list deletes every projected relation for
/// that event (the old `seed_relations` early return left them all behind).
#[tokio::test(flavor = "multi_thread")]
async fn emptying_an_events_relations_deletes_every_row() {
    let (pool, name) = fresh_migrated_db().await;
    seed_procedures(&pool, &["100"]).await;

    let with_relation = taxonomy(
        vec![category("cat", "Categoría", 1)],
        vec![event("ev", "cat", vec![relation("100", 1)])],
    );
    seed_taxonomy(&pool, &with_relation)
        .await
        .expect("first seed");
    assert_eq!(relation_external_ids(&pool, "ev").await, vec!["100"]);

    let emptied = taxonomy(
        vec![category("cat", "Categoría", 1)],
        vec![event("ev", "cat", Vec::new())],
    );
    let report = seed_taxonomy(&pool, &emptied).await.expect("second seed");

    assert!(
        relation_external_ids(&pool, "ev").await.is_empty(),
        "an event whose YAML relations are emptied must project zero relations"
    );
    assert_eq!(
        report.relations_removed, 1,
        "emptying the list removes and accounts for the stale relation"
    );

    common::drop_test_db(&name).await;
}

/// (c) An event and a category removed from the YAML disappear from the
/// projection and from the read paths. The category is removed only after its
/// events, which is the FK order `life_events.category_id` requires.
#[tokio::test(flavor = "multi_thread")]
async fn an_event_and_category_removed_from_yaml_disappear() {
    let (pool, name) = fresh_migrated_db().await;

    let both = taxonomy(
        vec![category("cat-a", "A", 1), category("cat-b", "B", 2)],
        vec![
            event("ev-a", "cat-a", Vec::new()),
            event("ev-b", "cat-b", Vec::new()),
        ],
    );
    seed_taxonomy(&pool, &both).await.expect("first seed");

    assert!(category_slugs(&pool).await.contains(&"cat-b".to_string()));
    assert!(event_exists(&pool, "ev-b").await);
    let listed = events_by_category(&pool, "cat-b")
        .await
        .expect("read path")
        .expect("known category");
    assert!(listed.iter().any(|row| row.slug == "ev-b"));

    let reduced = taxonomy(
        vec![category("cat-a", "A", 1)],
        vec![event("ev-a", "cat-a", Vec::new())],
    );
    let report = seed_taxonomy(&pool, &reduced).await.expect("second seed");

    assert_eq!(
        report.events_removed, 1,
        "the removed event is accounted for in the report"
    );
    assert_eq!(
        report.categories_removed, 1,
        "the removed category is accounted for in the report"
    );
    assert!(
        !event_exists(&pool, "ev-b").await,
        "the event removed from the YAML must leave the projection"
    );
    assert!(
        !category_slugs(&pool).await.contains(&"cat-b".to_string()),
        "the category removed from the YAML must leave the projection"
    );
    assert!(
        events_by_category(&pool, "cat-b")
            .await
            .expect("read path")
            .is_none(),
        "the removed category must no longer be readable"
    );
    assert!(
        events_by_category(&pool, "cat-a")
            .await
            .expect("read path")
            .expect("known category")
            .iter()
            .any(|row| row.slug == "ev-a"),
        "the retained category and event stay readable"
    );

    common::drop_test_db(&name).await;
}

/// (d) Re-seeding the same taxonomy twice changes nothing the second time:
/// zero inserts, zero updates, zero removals, byte-stable order.
#[tokio::test(flavor = "multi_thread")]
async fn re_seeding_the_same_taxonomy_is_idempotent() {
    let (pool, name) = fresh_migrated_db().await;
    seed_procedures(&pool, &["100"]).await;

    let stable = taxonomy(
        vec![category("cat", "Categoría", 1)],
        vec![event("ev", "cat", vec![relation("100", 1)])],
    );
    seed_taxonomy(&pool, &stable).await.expect("first seed");
    let second = seed_taxonomy(&pool, &stable).await.expect("second seed");

    assert_eq!(second.categories_inserted, 0);
    assert_eq!(second.categories_updated, 0);
    assert_eq!(second.events_inserted, 0);
    assert_eq!(second.events_updated, 0);
    assert_eq!(second.keywords_inserted, 0);
    assert_eq!(second.keywords_removed, 0);
    assert_eq!(second.synonyms_inserted, 0);
    assert_eq!(second.synonyms_removed, 0);
    assert_eq!(second.relations_written, 0);
    assert_eq!(second.relations_removed, 0);
    assert_eq!(second.relations_pending, 0);
    assert_eq!(second.events_removed, 0);
    assert_eq!(second.categories_removed, 0);
    assert_eq!(
        relation_external_ids(&pool, "ev").await,
        vec!["100"],
        "the relation set is byte-stable across re-seeds"
    );

    common::drop_test_db(&name).await;
}

/// FK observation the reconciliation order depends on: `life_events`
/// references `categories(id)` with no ON DELETE rule, so a category with a
/// live event cannot be deleted before its events (SQLSTATE 23503). The
/// projection reconciliation removes events first for exactly this reason.
#[tokio::test(flavor = "multi_thread")]
async fn a_category_with_a_live_event_cannot_be_deleted_directly() {
    let (pool, name) = fresh_migrated_db().await;
    let seeded = taxonomy(
        vec![category("cat", "Categoría", 1)],
        vec![event("ev", "cat", Vec::new())],
    );
    seed_taxonomy(&pool, &seeded).await.expect("seed");

    let error = sqlx::query("DELETE FROM categories WHERE slug = 'cat'")
        .execute(&pool)
        .await
        .expect_err("the category is still referenced by its event");
    assert!(
        matches!(&error, sqlx::Error::Database(db) if db.code().as_deref() == Some("23503")),
        "the category delete is blocked by the life_events FK: {error:?}"
    );

    common::drop_test_db(&name).await;
}

/// (e) An obsolete event that still has projected relations is removed and
/// its cascaded `life_event_procedures` rows are counted in
/// `relations_removed`: the FK is ON DELETE CASCADE, so the rows disappear
/// with the event and the report accounts for each relation row.
#[tokio::test(flavor = "multi_thread")]
async fn a_removed_event_counts_its_cascaded_relations() {
    let (pool, name) = fresh_migrated_db().await;
    seed_procedures(&pool, &["100", "200"]).await;

    let full = taxonomy(
        vec![category("cat", "Categoría", 1)],
        vec![
            event("keep", "cat", Vec::new()),
            event(
                "retire",
                "cat",
                vec![relation("100", 1), relation("200", 2)],
            ),
        ],
    );
    seed_taxonomy(&pool, &full).await.expect("first seed");
    assert_eq!(
        relation_external_ids(&pool, "retire").await,
        vec!["100", "200"],
        "the obsolete event starts with both relations projected"
    );

    let reduced = taxonomy(
        vec![category("cat", "Categoría", 1)],
        vec![event("keep", "cat", Vec::new())],
    );
    let report = seed_taxonomy(&pool, &reduced).await.expect("second seed");

    assert_eq!(report.events_removed, 1, "the obsolete event is removed");
    assert_eq!(
        report.relations_removed, 2,
        "the removed event's cascaded relations are counted in the report"
    );
    assert!(
        !event_exists(&pool, "retire").await,
        "the obsolete event leaves the projection"
    );
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM life_event_procedures")
        .fetch_one(&pool)
        .await
        .expect("count relations after the event delete");
    assert_eq!(remaining, 0, "no relation row survives the removed event");

    common::drop_test_db(&name).await;
}
