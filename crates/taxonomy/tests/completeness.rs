//! Loader completeness (TX-1, task 25): a single event YAML yields slug,
//! name, description, category, typed keywords, negative keywords,
//! ACTION_ENTITY rules, and positive/negative tests — with no code-level
//! event definition anywhere in `crates/taxonomy`.

use taxonomy::loader::load_data_dir;
use taxonomy::model::KeywordType;

fn valid_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid")
}

#[test]
fn single_event_yaml_yields_the_full_model() {
    let taxonomy = load_data_dir(&valid_dir()).expect("valid fixture must load");
    assert_eq!(taxonomy.events.len(), 1, "one event file, one event");
    let source = &taxonomy.events[0];
    assert_eq!(source.file, "events/comprar-vehiculo.yaml");
    let event = &source.event;

    assert_eq!(event.slug, "comprar-vehiculo");
    assert_eq!(event.name, "Comprar un vehículo");
    assert_eq!(
        event.description,
        "Trámites para comprar un vehículo nuevo o usado en Uruguay."
    );
    assert_eq!(event.category, "vehiculos");

    // Typed keywords: ACTION, ENTITY, MODIFIER all present, plus the
    // negative keyword (vender −15) with its declared weight.
    assert!(
        event.keywords.iter().any(|k| k.term == "comprar"
            && k.keyword_type == KeywordType::Action
            && k.weight == 10)
    );
    assert!(
        event.keywords.iter().any(|k| k.term == "vehiculo"
            && k.keyword_type == KeywordType::Entity
            && k.weight == 8)
    );
    assert!(
        event
            .keywords
            .iter()
            .any(|k| k.term == "usado" && k.keyword_type == KeywordType::Modifier && k.weight == 3)
    );
    let negative = event
        .keywords
        .iter()
        .find(|k| k.negative)
        .expect("negative keyword must load");
    assert_eq!(negative.term, "vender");
    assert_eq!(negative.weight, 15);

    // ACTION_ENTITY rules load as declared.
    assert_eq!(event.rules.len(), 1);
    assert_eq!(event.rules[0].action, "comprar");
    assert_eq!(event.rules[0].entity, "vehiculo");
    assert_eq!(event.rules[0].bonus, 15);

    // Positive/negative query tests load as declared.
    assert_eq!(event.tests.positive, vec!["compre un auto usado"]);
    assert_eq!(event.tests.negative, vec!["vendi mi auto"]);

    // Relations carry order and required as declared.
    assert_eq!(event.relations.len(), 2);
    assert_eq!(event.relations[0].external_id, "proc-001");
    assert_eq!(event.relations[0].order, 1);
    assert!(event.relations[0].required);
    assert_eq!(event.relations[1].external_id, "proc-002");
    assert_eq!(event.relations[1].order, 2);
    assert!(!event.relations[1].required);
}

#[test]
fn categories_and_synonyms_load_from_their_directories() {
    let taxonomy = load_data_dir(&valid_dir()).expect("valid fixture must load");
    assert_eq!(taxonomy.categories.len(), 1);
    assert_eq!(taxonomy.categories[0].file, "categories/vehiculos.yaml");
    assert_eq!(taxonomy.categories[0].category.slug, "vehiculos");
    assert_eq!(taxonomy.categories[0].category.order_index, 1);

    assert_eq!(taxonomy.synonyms.len(), 2);
    let terms: Vec<(&str, &str)> = taxonomy
        .synonyms
        .iter()
        .map(|s| (s.synonym.term.as_str(), s.synonym.canonical.as_str()))
        .collect();
    assert!(terms.contains(&("auto", "vehiculo")));
    assert!(terms.contains(&("coche", "vehiculo")));
}

#[test]
fn no_code_level_event_definition_exists_in_the_crate() {
    // Events exist only as YAML (TX-1): no domain slugs, keyword terms, or
    // seed vocabulary may appear in `crates/taxonomy/src`. Comments are
    // stripped first so documentation about the rule never trips the scan.
    const FORBIDDEN_DOMAIN_TOKENS: [&str; 8] = [
        "comprar",
        "vender",
        "vehiculo",
        "patente",
        "libreta",
        "matricula",
        "transferir",
        "accidente",
    ];

    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut stack = vec![src_dir.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("src must be readable") {
            let path = entry.expect("src entry must be readable").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("src file must be readable");
            for (index, line) in text.lines().enumerate() {
                // Strip line comments (covers `//`, `///`, `//!` doc comments).
                let code = line.split("//").next().unwrap_or("");
                for token in FORBIDDEN_DOMAIN_TOKENS {
                    if code.contains(token) {
                        offenders.push(format!(
                            "{}:{}: {token}",
                            path.strip_prefix(&src_dir).unwrap_or(&path).display(),
                            index + 1
                        ));
                    }
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "code-level event definitions are forbidden (events exist only as YAML): {offenders:?}"
    );
}
