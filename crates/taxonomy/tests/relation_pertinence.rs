//! Relation pertinence (audit finding F1, WU-1 T1): a ranking test alone
//! cannot catch a wrong event→procedure relation — `comprar-vehiculo` can
//! rank TOP1 and still recommend a procedure that does not apply to a
//! citizen buyer. This test loads the real `data/events` directory and
//! asserts the curated pertinence table below.
//!
//! Every id was verified first-hand against the live composed catalog
//! (`procedures` + `organizations`) on 2026-09-22. Official evidence per
//! forbidden id:
//!
//! - `4551` "Solicitud de empadronamientos" — **Dirección Nacional de
//!   Catastro**: "empadronar parcelas, generadas a partir de planos de
//!   mensura inscriptos ...". It is a parcel-survey procedure, not a
//!   vehicle registration.
//! - `2368` "Alta de vehículos ante la Dirección Nacional de Transporte
//!   (DNT)" — **Dirección Nacional de Transporte**: "inscripción por
//!   primera vez ... de todos los vehículos de Carga (mayores o iguales a
//!   2000 kg ... o 3500 kg de Peso Bruto Total) y Vehículos de pasajeros,
//!   a partir de 8 asientos". It cannot justify generic buyer applicability.
//! - `6995` "Registro de Automotoras o Gestoría para Empadronamiento de
//!   Vehículos" — **Dirección General de Tránsito**: "una Automotora o
//!   Gestoría solicita el alta para efectuar empadronamientos". It is an
//!   alta for dealerships/gestorías, not a citizen-buyer step.
//! - `2198` "Renovación del permiso nacional de circulación y cédula de
//!   identificación vehicular" — **Dirección Nacional de Transporte**:
//!   "habilita a los vehículos que pertenecen a Empresas No Profesionales
//!   de Carga, Empresas de pasajeros Regulares y No Regulares (Turismo,
//!   Oficial y Propio) y Cédula de Identificación para Empresas
//!   Profesionales de Carga". Company-oriented, not the generic citizen
//!   buyer flow.
//! - `4327` "Duplicado de libreta de propiedad o documento de identificación
//!   vehicular (DIV) por extravío o hurto - Paysandú" — **Intendencia**
//!   (Paysandú): "obtener un duplicado del documento de identificación del
//!   vehículo (libreta) cuando se ha extraviado o resultó hurtado". It is the
//!   vehicle's ownership/registration document, not the driver's license.
//!
//! Required relations are the steps that actually apply to a citizen query
//! (verified titles in the same pass):
//!
//! - `perder-libreta` -> `4428` "Duplicado de licencia de conducir (por
//!   Extravío o Hurto) - Paysandú" — **Intendencia** (Paysandú): "obtener un
//!   duplicado del carné de Licencias de Conducir cuando se lo ha extraviado
//!   o ha sido hurtado". This is the genuine driver's-license duplicate.
//! - `6978` "Empadronamiento de vehículos - Canelones" — **Dirección General
//!   de Tránsito**: "Trámite por el cual el adquiriente de un vehículo
//!   gestiona el empadronamiento ... otorgándose matrícula y documento de
//!   identificación vehicular" (subcase `6978-1` covers autos, camionetas,
//!   motos y similares).
//! - `6980` "Cambio de titularidad de vehículo (Transferencia) - Canelones"
//!   — **Dirección General de Tránsito**: used-vehicle transfer of title.
//!   Same department as `6978`, so a single citizen is never sent to two
//!   Intendencias.
//!
//! `6182` (Maldonado 0 Km) is a real, applicable empadronamiento but is not
//! chosen here: pairing it with a Canelones transfer mixes departments.
//!
//! `renovar-cedula`'s optional `5800` ("Desbloqueo de PIN y renovación de
//! certificado de firma de cédula de identidad electrónica") is a related
//! electronic-DNI step; it is intentionally left declared and optional, and
//! is not listed as forbidden.

use std::collections::HashSet;
use std::path::Path;

use taxonomy::loader::load_data_dir;
use taxonomy::model::EventSource;
use taxonomy::validator::validate_dir_against_snapshot;

/// Repo root: the pertinence test runs against the real committed seed.
fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
}

/// Loads and validates the real seed exactly as the taxonomy-validate CLI
/// does, so a pertinence failure is never masked by a broken seed.
fn load_validated_events() -> Vec<EventSource> {
    let data_dir = repo_root().join("data");
    let snapshot = repo_root().join("data/external_ids.snapshot.txt");
    let errors = validate_dir_against_snapshot(&data_dir, &snapshot);
    assert!(
        errors.is_empty(),
        "the real seed must validate with zero errors, got: {errors:?}"
    );
    load_data_dir(&data_dir)
        .expect("the real seed loads through the taxonomy loader")
        .events
}

/// One relation id an event must NOT declare, with its verified reason.
struct ForbiddenRelation {
    event: &'static str,
    external_id: &'static str,
    reason: &'static str,
}

/// One relation id an event MUST declare, with its verified reason.
struct RequiredRelation {
    event: &'static str,
    external_id: &'static str,
    reason: &'static str,
}

/// Curated pertinence table (F1). Add an entry only with catalog evidence.
const FORBIDDEN_RELATIONS: &[ForbiddenRelation] = &[
    ForbiddenRelation {
        event: "comprar-vehiculo",
        external_id: "4551",
        reason: "Dirección Nacional de Catastro: empadronamiento de parcelas \
                 a partir de planos de mensura, not a vehicle registration",
    },
    ForbiddenRelation {
        event: "comprar-vehiculo",
        external_id: "2368",
        reason: "DNT: alta limited to cargo vehicles >=2000 kg / >=3500 kg PBT \
                 and passenger vehicles from 8 seats, not the generic buyer flow",
    },
    ForbiddenRelation {
        event: "comprar-vehiculo",
        external_id: "6995",
        reason: "Dirección General de Tránsito: alta for automotoras or gestorías, \
                 not a citizen-buyer step",
    },
    ForbiddenRelation {
        event: "comprar-vehiculo",
        external_id: "2198",
        reason: "DNT: circulation permit / vehicle id document addressed to \
                 Empresas No Profesionales de Carga and passenger-transport \
                 companies, not the generic citizen buyer flow",
    },
    ForbiddenRelation {
        event: "perder-libreta",
        external_id: "4327",
        reason: "Paysandú: duplicate of the vehicle's ownership/registration \
                 document (libreta de propiedad / DIV), not the driver's license",
    },
];

/// Curated required-relation table (F1). Add an entry only with catalog
/// evidence.
const REQUIRED_RELATIONS: &[RequiredRelation] = &[
    RequiredRelation {
        event: "comprar-vehiculo",
        external_id: "6978",
        reason: "new-vehicle registration (Empadronamiento de vehículos - Canelones)",
    },
    RequiredRelation {
        event: "comprar-vehiculo",
        external_id: "6980",
        reason: "used-vehicle transfer of title (Cambio de titularidad - Canelones)",
    },
    RequiredRelation {
        event: "perder-libreta",
        external_id: "4428",
        reason: "driver's-license duplicate (Duplicado de licencia de conducir - Paysandú)",
    },
];

/// Exact required-id set for `comprar-vehiculo` (F1 correction): a re-added
/// `2198` — required or optional — must fail the suite. The forbidden entry
/// above rejects the optional form; this exact set rejects the required one.
const COMPRAR_VEHICULO_REQUIRED_IDS: [&str; 2] = ["6978", "6980"];

/// Exact required-id set for `perder-libreta`: the genuine driver's-license
/// duplicate, not the vehicle ownership/registration document.
const PERDER_LIBRETA_REQUIRED_IDS: [&str; 1] = ["4428"];

fn declared_relation_ids(event: &EventSource) -> HashSet<&str> {
    event
        .event
        .relations
        .iter()
        .map(|relation| relation.external_id.as_str())
        .collect()
}

fn required_relation_ids(event: &EventSource) -> HashSet<&str> {
    event
        .event
        .relations
        .iter()
        .filter(|relation| relation.required)
        .map(|relation| relation.external_id.as_str())
        .collect()
}

fn find_event<'a>(events: &'a [EventSource], slug: &str) -> &'a EventSource {
    events
        .iter()
        .find(|source| source.event.slug == slug)
        .unwrap_or_else(|| panic!("the real seed must declare event {slug}"))
}

/// F1: no event may declare a relation whose official procedure does not
/// apply to the event's audience. The curated table carries one verified
/// reason per forbidden id.
#[test]
fn forbidden_relations_are_not_declared() {
    let events = load_validated_events();
    let mut checked = 0usize;
    for entry in FORBIDDEN_RELATIONS {
        let event = find_event(&events, entry.event);
        let declared = declared_relation_ids(event);
        assert!(
            !declared.contains(entry.external_id),
            "{} must not declare relation {} ({})",
            entry.event,
            entry.external_id,
            entry.reason
        );
        checked += 1;
    }
    assert!(checked > 0, "the pertinence table must list forbidden ids");
}

/// F1: every citizen-applicable step the curated table records must be
/// declared by its event.
#[test]
fn required_relations_are_declared() {
    let events = load_validated_events();
    let mut checked = 0usize;
    for entry in REQUIRED_RELATIONS {
        let event = find_event(&events, entry.event);
        let declared = declared_relation_ids(event);
        assert!(
            declared.contains(entry.external_id),
            "{} must declare relation {} ({})",
            entry.event,
            entry.external_id,
            entry.reason
        );
        checked += 1;
    }
    assert!(checked > 0, "the pertinence table must list required ids");
}

/// F1 correction: `comprar-vehiculo`'s required-id set is exactly the
/// verified new/used-vehicle steps, so a re-added `2198` fails.
#[test]
fn comprar_vehiculo_required_set_is_exact() {
    let events = load_validated_events();
    let event = find_event(&events, "comprar-vehiculo");
    let required = required_relation_ids(event);
    let expected: HashSet<&str> = COMPRAR_VEHICULO_REQUIRED_IDS.iter().copied().collect();
    assert_eq!(
        required, expected,
        "comprar-vehiculo must require exactly {expected:?}, got {required:?}"
    );
}

/// F1 correction: `perder-libreta`'s required-id set is exactly the genuine
/// driver's-license duplicate, not the vehicle DIV (`4327`).
#[test]
fn perder_libreta_required_set_is_exact() {
    let events = load_validated_events();
    let event = find_event(&events, "perder-libreta");
    let required = required_relation_ids(event);
    let expected: HashSet<&str> = PERDER_LIBRETA_REQUIRED_IDS.iter().copied().collect();
    assert_eq!(
        required, expected,
        "perder-libreta must require exactly {expected:?}, got {required:?}"
    );
}
