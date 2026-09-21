# Lote 17: UTE alta/baja, facturas UTE/OSE y permiso de feriante

## Intent
Cover UTE electricity service connection/disconnection and installation
inspection, online utility bill payment for UTE and OSE (including chosen due
date), and street-vendor (feriante) permits, stall transfers, and market
changes handled by the intendencias.

## Verified relations (external_ids verified in
`data/external_ids.snapshot.txt`; names confirmed in local catalog)
- alta-o-baja-electrica-ute: 1408 (Solicitud de alta del servicio eléctrico -
  UTE, required), 1387 (Solicitud de baja del servicio eléctrico - UTE,
  required), 1418 (Solicitud de retiro provisorio y definitivo de
  instalaciones - UTE, optional).
- pagar-factura-ute-ose: 1380 (Pago de facturas de UTE por internet - UTE,
  required), 1386 (Pagos a la cuenta - UTE, optional), 1404 (Modalidades de
  pago - UTE, optional), 1400 (Solicitud de cambio de fecha de vencimiento de
  la factura (vencimiento elegido) - UTE, optional), 1619 (Solicitud de cambio
  de fecha de vencimiento de la factura - OSE, optional).
- permiso-feriante: 2695 (Solicitud de Calidad de Feriante o Baja de
  Permisionario, required), 2693 (Solicitud de cambio de feria, optional),
  2684 (Traspasos de puestos de ferias, optional), 5164 (Solicitud de permisos
  de ferias - Florida, optional), 6229 (Permiso feriantes - Lavalleja,
  optional).

## Constraints
- YAML is the source of truth; every event embeds positive/negative tests.
- pagar-factura-ute-ose must not absorb patente/multa payment queries: those
  belong to pagar-patente / pagar-multa-transito, which pair the pagar action
  with patente/multa rules; separation relies on factura/ute/ose tokens.
- alta/baja appear as ENTITY-typed keywords but are referenced as action terms
  in the rules; the matcher treats term types loosely, so this is intentional.
- Expected validation counts: 88 events / 14 categories / 28 synonyms.

## Tasks
- [x] T1: verify catalog IDs and descriptions.
- [x] T2: write 3 event files (delegated writer).
  - Evidence: 2026-09-21: delegated writer created the three lote-17 event
    files (deviations: none).
- [ ] T3: golden cases + full verification suite.
  - Evidence: 2026-09-21: negative patente (8) added to pagar-factura-ute-ose;
    standalone patente queries restored.
- [ ] T4: seed, end-to-end check, gga-reviewed commit.
