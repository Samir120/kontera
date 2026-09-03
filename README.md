# Kontera

> *kontera* (v.) — Swedish, "to assign accounts to a transaction". Deciding which
> accounts a transaction posts to is the operation this library exists to
> perform; everything else is downstream of it.
>
> Name verified free on crates.io and npm as of 2026-09-03. Reserve both before
> writing code — squatting is cheap and renaming later is not.

**Swedish bookkeeping as a library.** Feed it a commerce event log, get back
balanced verifications on BAS accounts and a SIE 4I file your accountant can
import into Fortnox or Visma.

```rust
let ledger = kontera::post(&events, &config)?;
let sie    = kontera_sie::write_4i(&ledger)?;
```

No database. No server. No admin UI. One pure function and an output file.

---

## The problem

A Swedish webshop takes 40 orders on Monday for 50,000 kr including 25% moms.
Two customers return items worth 1,000 kr. Stripe charges 1,168 kr in fees. On
Thursday, one line appears on the bank statement:

**47,832 kr**

That number is all the bank feed knows about Monday. The bookkeeping it has to
produce is nine postings across five accounts, and afterwards the payment
provider receivable account must be *exactly zero*.

Three things go wrong, and they are all the same mistake:

1. **The payout is booked as revenue.** Revenue becomes 47,832 instead of
   40,000. Fees disappear. Output VAT is understated by roughly 1,600 kr.
2. **Timing drifts.** Sales happened Monday, cash landed Thursday. Across a
   month boundary, the momsdeklaration is wrong.
3. **Nothing checks.** There is no built-in assertion that
   `payout == sales − refunds − fees`, so errors accumulate silently until
   someone reconciles by hand at year-end.

## What this is

A pure Rust library that converts commerce events into Swedish bookkeeping and
**refuses to emit output it cannot prove is correct**.

```
IN                        OUT
──────────────────        ─────────────────────────
SaleCaptured              Balanced verifications
RefundIssued       ──►    on BAS accounts
FeeCharged                        │
PayoutSettled                     ▼
+ account/VAT config      SIE 4I file (.si)
```

Three properties define it:

- **It enforces the settlement invariant.** `payout == sales − refunds − fees`.
  When that fails, you get an `Err`, not a plausible-looking wrong answer.
  Unbalanced verifications are unrepresentable in the type system.
- **It speaks Swedish.** BAS chart of accounts, moms at 25/12/6, EU reverse
  charge, OSS, export. Output is SIE 4I — the format every Swedish accounting
  system accepts.
- **It embeds.** One core, three shells: a CLI, an npm package via WASM, and an
  HTTP sidecar. `npm install` and call a function. No integration licence, no
  external service, no merchant data leaving the host's server.

## Who it's for

Not merchants. **Platform builders** — the person shipping a Swedish webshop who
wants correct bookkeeping inside their own admin panel, and a clean SIE file for
the merchant's accountant at year-end.

> Stripe gives you payments as a library.
> Nobody gives you Swedish bookkeeping as a library. This does.

## What it is not

Not an accounting platform. Not an invoicing system. Not a Fortnox replacement.
See [`docs/01-problem-and-scope.md`](docs/01-problem-and-scope.md) for the full
non-goals list, which is the most important document in this repository.

## Status

Pre-alpha. Nothing works yet. See [`docs/06-roadmap.md`](docs/06-roadmap.md).

## Documentation

[`STATE.md`](STATE.md) is the living project state — read it first.

| Document | Purpose |
|---|---|
| [01 Problem & Scope](docs/01-problem-and-scope.md) | Problem statement, non-goals, success criteria |
| [02 Domain Model](docs/02-domain-model.md) | Accounting concepts, BAS mapping, VAT scenarios, glossary |
| [03 Architecture](docs/03-architecture.md) | Crate layout, purity boundary, integration targets |
| [04 Public API](docs/04-public-api.md) | Event schema, config schema, error taxonomy, output contract |
| [05 Testing Strategy](docs/05-testing-strategy.md) | Property tests, golden files, the acceptance test |
| [06 Roadmap](docs/06-roadmap.md) | Ten-week plan with exit criteria |
| [07 Release Engineering](docs/07-release-engineering.md) | Versioning, CI, publishing, licensing |
| [08 Coding Standards](docs/08-coding-standards.md) | Rust conventions for this codebase |
| [09 Working Agreement](docs/09-working-agreement.md) | Session protocol, Definition of Done, review checklist |
| [Project Instructions](PROJECT-INSTRUCTIONS.md) | Paste into the Claude Project settings |
| [STATE.md](STATE.md) | Where the project actually is, right now |
| [ADRs](docs/adr/) | Architecture decision records |

## Disclaimer

This software produces accounting data. It is not accounting advice, and its
authors are not accountants or tax advisers. Output must be reviewed by a
qualified person before it is used in a filing to Skatteverket. See
[`docs/01-problem-and-scope.md`](docs/01-problem-and-scope.md#regulatory-posture).

## Licence

TBD — see [`docs/07-release-engineering.md`](docs/07-release-engineering.md#licensing).
