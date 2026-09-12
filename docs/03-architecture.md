# 03 — Architecture

**Status:** draft · **Owner:** Samir · **Last reviewed:** 2026-09-03

---

## 1. The one idea

```rust
pub fn post(events: &[Event], config: &Config) -> Result<Ledger, PostingError>;
```

A pure function. No I/O, no async, no database, no clock, no randomness. Same
input, same output, forever.

Everything else in this architecture is a consequence of that signature.

## 2. Consequences of purity

| Because the core is pure… | We get |
|---|---|
| No state to corrupt | No migrations, no sync bugs, no "the ledger drifted" incidents |
| No I/O | Property testing over generated event sequences is possible at all |
| No async | Compiles to WASM without a runtime; usable from any host |
| No storage | The host already stores orders; it stores events too. Zero operational burden |
| Deterministic | Golden-file tests can compare bytes, not shapes |

The host is the source of truth. The library is a calculation. If output is
wrong, you fix the code and re-run — there is nothing to repair.

## 3. Workspace layout

A Cargo workspace. Each crate needs its own `Cargo.toml` and its sources under
`src/` — Cargo will not find modules placed at the crate root.

```
kontera/
├── Cargo.toml                      # [workspace] members, shared deps
├── Cargo.lock                      # committed (07 §1)
├── .gitignore
├── .gitattributes                  # *.si is binary — CP437, do not normalise
├── crates/
│   ├── kontera-core/               # pure. no I/O, no async, no unsafe
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs              # mod declarations + pub use + post()
│   │   │   ├── money.rs            # Money, rounding
│   │   │   ├── account.rs          # AccountNumber, AccountKind
│   │   │   ├── verification.rs     # BalancedTransaction (private ctor)
│   │   │   ├── event.rs            # the four events
│   │   │   ├── config.rs           # account mapping, VAT profile, periods
│   │   │   ├── vat.rs              # scenario determination
│   │   │   ├── rules.rs            # event -> verification
│   │   │   ├── settle.rs           # the I2 invariant
│   │   │   ├── ledger.rs           # numbering, assembly, reports
│   │   │   └── error.rs            # PostingError
│   │   └── tests/
│   │       └── properties.rs       # proptest suite (05 §2)
│   ├── kontera-testkit/            # generators shared across test crates
│   │   ├── Cargo.toml
│   │   └── src/lib.rs              # valid_event_log(), fixtures builders
│   ├── kontera-sie/                # SIE 4I writer
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── cp437.rs            # encoder; Err, never substitution
│   │   │   └── writer.rs           # #VER / #TRANS / header records
│   │   └── tests/golden.rs
│   ├── kontera-cli/
│   │   ├── Cargo.toml
│   │   └── src/main.rs
│   └── kontera-wasm/
│       ├── Cargo.toml
│       └── src/lib.rs              # wasm-bindgen bindings
├── npm/                            # npm package wrapping the wasm output
├── fixtures/                       # golden test data
├── scripts/
│   └── check-purity.sh             # 07 §3.1
├── .github/workflows/
└── docs/
```

Unit tests live beside the code in `#[cfg(test)] mod tests`. `tests/` holds
integration tests, which exercise only the public API — a useful forcing function
for keeping that API usable.

### 3.1 Why `kontera-testkit` is a crate, not a test module

Each file under `tests/` compiles as its own crate, so a generator defined in
`tests/gen.rs` cannot be imported by `tests/properties.rs`. And `kontera-sie`'s
golden tests need generated ledgers too, across a crate boundary.

The alternatives are `tests/common/mod.rs` duplicated per crate, or a
`testing` feature flag on `kontera-core` that exposes generators from the library
itself. The feature-flag approach puts test scaffolding in the shipped crate's
public surface, which is exactly the kind of thing that later gets depended on by
accident. A separate crate, never published, keeps the boundary clean.

### 3.2 Why `kontera-sie` is separate

SIE needs CP437 encoding, which means a dependency. `kontera-core` should stay
dependency-light (`rust_decimal`, `serde`, `chrono` or `time`, and little else)
so it compiles fast, audits cleanly, and drops into WASM without weight.

It also keeps the seam honest: output format is a serialisation concern, not a
domain concern. A future SIE 5 writer or a Fortnox-API journal writer slots in
beside it without touching the core.

## 4. Data flow

```
   host platform
        │
        │  (its own order model)
        ▼
  ┌─────────────────┐
  │ adapter         │   anti-corruption layer — lives in the HOST, not here
  │ Order -> Event  │
  └─────────────────┘
        │
        │  Vec<Event>  +  Config
        ▼
  ┌──────────────────────────────────────────┐
  │ kontera-core::post()                      │
  │                                          │
  │  validate ──► vat ──► rules ──► settle   │
  │      │         │        │         │      │
  │      └─────────┴────────┴─────────┘      │
  │              PostingError                │
  │                    │                     │
  │                 Ledger                   │
  └──────────────────────────────────────────┘
        │
        ├──► kontera-sie::write_4i()  ──► .si file
        └──► Ledger (serde)          ──► host renders huvudbok, momsrapport
```

### 4.1 Pipeline stages

1. **validate** — duplicate IDs, dangling references, dates inside the fiscal
   year, closed periods
2. **vat** — determine one of four scenarios per sale line; unknown → `Err`
3. **rules** — map each event to a `BalancedTransaction` using `Config`
4. **settle** — group by payout, assert I2, emit the payout verification
5. **assemble** — sort deterministically, assign verification numbers, build
   reports

Stages are separate functions with their own tests. `post()` is the composition.

### 4.2 Determinism rules

Determinism is a *property*, and properties need enforcing:

- Sort by `(date, event_id)` before numbering. Never rely on input order.
- No `HashMap` iteration in output paths. `BTreeMap` only.
- No `SystemTime::now()` anywhere in `core` or `sie`. The `#GEN` date in the SIE
  header is passed in by the caller, not read from the clock — otherwise golden
  tests can't compare bytes.

That last one is easy to get wrong and will be caught by the golden tests on day
one, which is the point.

## 5. The anti-corruption layer

The adapter that turns a SwadeStack order into a `kontera::Event` lives **in
SwadeStack**, not in this repository.

This matters. If the library grows a `swadestack` feature flag, it stops being a
library. The contract is the event schema — versioned, documented in
[04](04-public-api.md), and the only thing either side may depend on.

### 5.1 Naming rule

**No host or organisation name appears anywhere in this repository** — not in the
crate name, the npm scope, config examples, fixtures, or docs. The library is
built *by* SwadeStack developers and integrated *into* SwadeStack first, but it is
not a SwadeStack component, and naming it as one forecloses every other host
before the first release.

Config examples use a clearly fictional company (`Exempelbutiken AB`) because
examples get copy-pasted, and a real org number in a public repository is a
mistake that's hard to take back.

## 6. Integration targets

One core, three shells, built in this order.

### 6.1 CLI — week 8

```
kontera replay events.jsonl --config kontera.toml --out 2026-01.si
```

No host needed. Demoable to an accountant before any integration exists. This is
also how the golden tests run.

### 6.2 npm package via WASM — week 10

```js
import { post, writeSie } from 'kontera';
const ledger = post(events, config);
```

`wasm-bindgen` → `wasm-pack` → npm. No sidecar, no container, no network hop, no
auth layer. For a Node host this is the strongest integration story available,
and it exists only because the core is pure and sync.

The same artifact runs in the browser, so a host admin panel can show a live
posting preview as someone edits an order — no round-trip.

### 6.3 HTTP sidecar — only if .NET needs it

Axum, one `POST /post` endpoint, stateless. Trivial once the core exists. Not
built unless a .NET host actually requires it. Do not build it speculatively.

## 7. What lives where

| Concern | Location | Rationale |
|---|---|---|
| Order storage | Host | Already exists |
| Event storage | Host | It's their data, and retention is their legal obligation |
| Account mapping config | Host, passed in | Per-merchant |
| Period open/closed | Host, passed in config | Core has no memory |
| Posting logic | `kontera-core` | The product |
| SIE serialisation | `kontera-sie` | Format concern |
| File writing | CLI / host | I/O |
| Admin UI | Host | Not our business |

## 8. Dependency policy

`kontera-core` may depend on: `rust_decimal`, `serde`, a date crate, `thiserror`.
Additions require a note in the PR explaining why. Every dependency is something
a future auditor has to read.

`kontera-core` may not: perform I/O, use `async`, use `unsafe`, call
`SystemTime::now()`, or panic on any input reachable from `post()`.

CI enforces the first three mechanically. See
[05](05-testing-strategy.md) and [07](07-release-engineering.md).

## 9. Extension points designed in now, built later

These are shapes the architecture leaves room for, not v0.1 work:

- **Second output writer** — Fortnox/Spiris API journals instead of a SIE file.
  Same `Ledger`, different serialiser. This is the hedge that means the project
  doesn't depend on displacing Fortnox.
- **Second posting strategy** — kontantmetoden as an alternative `rules` impl.
- **Rate table as data** — VAT rates supplied via `Config` so they can change
  without a release.

None of these are implemented in v0.1. They are listed so the seams are placed
correctly the first time.
