# ADR-0003 — Emit SIE 4I as CP437 bytes

**Status:** Accepted · **Date:** 2026-09-03

## Context

SIE has several types. Type 4 comes in two variants: 4E, a full export with
year-end balances and transactions, and 4I, transactions only — intended for a
*försystem* (a payroll or invoicing system) generating bookkeeping orders for
import into an accounting system.

The older SIE types use MS-DOS codepage 437, a character set the SIE Group's own
documentation describes as nearly extinct. SIE 5 is the XML successor with
support for attached electronic documents and XMLDsig signatures, but few
programs implement it.

## Decision

Emit **SIE 4I**, encoded as **CP437**, file extension `.si`.

`write_4i` returns `Vec<u8>`, not `String`, because CP437 output is not valid
UTF-8 and pretending otherwise pushes the problem to the caller.

The `#GEN` date is a function parameter, never read from the system clock —
otherwise golden-file tests cannot compare bytes.

## Consequences

**Good**

- 4I is exactly this library's role: a subsystem producing bookkeeping orders for
  someone else's ledger. Emitting 4E would be claiming to own a ledger we don't
- Universally supported by Swedish accounting software
- `Vec<u8>` makes the encoding boundary explicit

**Bad**

- CP437 is a genuine hazard. Everything works locally until the first `å` in a
  company name. This is risk R2 and has its own fixture
- Characters outside CP437 (an emoji in a product description, a Cyrillic
  customer name) are unrepresentable. The encoder returns
  `Err(UnrepresentableCharacter)` rather than substituting `?` — silent
  substitution in räkenskapsinformation is worse than a failed export
- No attachments, no signatures. Both are SIE 5 features

## Alternatives rejected

- **SIE 4E** — implies ownership of the ledger, including opening and closing
  balances we don't have.
- **SIE 5 (XML)** — technically nicer, adoption too thin. Revisit when receiving
  systems support it.
- **UTF-8 SIE 4** — non-conformant. Some importers tolerate it; relying on that
  is how you get a support queue.
