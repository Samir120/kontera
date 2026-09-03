# 09 — Working Agreement

**Status:** accepted · **Owner:** Samir · **Last reviewed:** 2026-09-03

How Kontera gets built. `PROJECT-INSTRUCTIONS.md` is the compressed version that
lives in the Claude Project settings; this is the full text, versioned with the
code.

---

## 1. The operating constraint

Kontera is built through Claude Project chats, not Claude Code. Everything below
follows from three facts:

| Fact | Consequence |
|---|---|
| Claude cannot see the repository | `STATE.md` and pasted files are its entire view of the codebase |
| Claude cannot compile or run tests | Samir is the sole oracle for "does it work" |
| Chats are independent | Continuity lives in `STATE.md`, not in Claude's memory |

The third one is the trap. Claude may recall fragments of earlier conversations,
and those fragments can be stale or wrong. **`STATE.md` outranks recollection.**
When they disagree, the file wins and the recollection is discarded.

## 2. Session protocol

### 2.1 Opening

Every chat opens with a three-line orientation from `STATE.md`: current
milestone, last completed task, next task. Then confirmation before work starts.

Suggested opening prompt:

```
Read STATE.md and orient. We're doing T4 this session.
```

If `STATE.md` is missing from project knowledge, Claude says so and stops. It
does not proceed on assumptions.

### 2.2 One task per chat

A chat covers one task from the current milestone's board. When the task is done,
the chat ends. Starting a second task in the same chat means the context now
holds two problems' worth of code, and quality degrades.

If a task turns out to be larger than one chat, split it in `STATE.md` and hand
off mid-task with a recorded next step.

### 2.3 Closing

Every session ends with a complete replacement `STATE.md` in one fenced block —
never a partial file, never "add this line". Samir saves it over the old file and
re-uploads to project knowledge.

A session that ends without a `STATE.md` update has lost its work.

## 3. Definition of Done

A task is done when **all** of these hold. Claude proposes done; Samir declares
it.

```
[ ] Code compiles                     — cargo build --workspace
[ ] Tests pass                        — cargo test --workspace
[ ] Clippy clean                      — cargo clippy --all-targets -- -D warnings
[ ] Formatted                         — cargo fmt --all --check
[ ] Purity check passes               — scripts/check-purity.sh
[ ] New behaviour has a test          — property test if it's an invariant
[ ] Public items documented           — /// on everything exported
[ ] Affected docs updated             — 02/03/04 if the domain, architecture or contract moved
[ ] VERIFY markers preserved          — none silently resolved
[ ] STATE.md updated                  — task board, session log, open questions
```

The first five are mechanical and Samir runs them. Claude never claims any of
them. The correct phrasing is: *"This should compile. Run `cargo test -p
kontera-core` and paste the output."*

## 4. The completion block

When the DoD is met, Claude emits exactly this shape:

```markdown
## ✅ T4 COMPLETE — BalancedTransaction

**Definition of Done**
- [x] Compiles — confirmed by you
- [x] Tests pass — 14 unit, 3 property
- [x] Clippy clean — confirmed by you
- [ ] Docs updated — 02 §2.3 still says "Vec<Line>", now "Lines"
...

**STATE.md** — replace the file with this:
```(fenced block)```

**Next: T5 — PostingError skeleton**
Why now: T5's variants are referenced by T4's constructor, currently stubbed.

Opening prompt for a new chat:
> Read STATE.md and orient. We're doing T5 this session.
```

If the DoD is not met, Claude says so and lists what is outstanding. **Marking a
task done to be agreeable is the single most damaging thing that can happen in
this workflow** — it puts `STATE.md` out of sync with reality, and every
subsequent session inherits the error.

## 5. Handling ambiguity

State the ambiguity in one sentence. Recommend one option with a one-sentence
reason. Proceed with the recommendation unless told otherwise.

Do not ask open questions ("how would you like to handle X?"). Do not present
five options. Do not relitigate a decision already recorded in `STATE.md` or an
ADR.

**Exception:** if the ambiguity is architectural — anything that would change a
public contract, add state, or affect the ledger's correctness — stop and propose
an ADR before writing code that depends on the answer.

## 6. Code review checklist

Applied by Claude to its own output before sending, and by Samir on receipt:

**Correctness**
- Does an invariant from `02` §6 apply here, and is it enforced in types rather
  than checked at runtime?
- Any `f64`? Any float in a test or fixture?
- Any `_ =>` wildcard on a domain enum?
- Any rounding that sums before rounding rather than after?

**Purity**
- I/O, async, `unsafe`, clock reads, `HashMap` in an output path?
- New dependency in `kontera-core`? Justified in the response?

**Domain**
- Any BAS number, VAT rate or box number not traceable to `02`?
- Any `VERIFY` marker silently resolved?
- Does an unsupported input fail closed with a typed error?

**Craft**
- `unwrap()`/`expect()` outside tests?
- Public item without `///`?
- A trait with one implementation? (Abstractions are earned by a second caller.)
- More than ~300 lines in one response? Split the task instead.

## 7. Pasting protocol

Because Claude cannot read the repo:

- **Claude asks for what it needs.** "Paste `crates/kontera-core/src/money.rs`."
  It does not invent the contents of an unseen file.
- **Claude returns complete files**, with the full path as the first line
  comment. Not fragments, not "…rest unchanged" — those turn into merge errors
  in an editor.
- **Samir pastes real compiler output**, not a summary of it. "It didn't work" is
  not debuggable; `error[E0308]` with the span is.

## 8. What lives where

| Artifact | Home | Updated |
|---|---|---|
| Project instructions | Claude Project settings | Re-paste when `PROJECT-INSTRUCTIONS.md` changes |
| Specification (`docs/01`–`09`, ADRs) | Project knowledge + repo | On decision changes |
| `STATE.md` | Project knowledge + repo | Every session |
| Source code | Repo only | Pasted into chat as needed |

**Source code does not go into project knowledge.** It changes every session, and
a stale copy in knowledge is worse than none — Claude would reason about code
that no longer exists.

## 9. Cadence

- **Every session:** update `STATE.md`.
- **Every milestone:** re-read `01` §4 non-goals. Scope creep is risk R5 and it
  arrives one reasonable-sounding request at a time.
- **Every milestone:** prune the `STATE.md` session log to the current milestone.
- **Monthly:** re-read `06` §risk register. Update what changed.

## 10. When Claude should refuse

Claude is expected to disagree, once, clearly, with a reason:

- Scope creep against `01` §4
- Anything adding state, I/O or async to the core (ADR-0001)
- An undefended dependency in `kontera-core`
- An abstraction with one implementation
- Marking a task done when the DoD is unmet
- Deferring the week-6 SIE acceptance test, which is the project's real gate

If overruled, record it in `STATE.md` under Decisions and move on. The record
matters more than the argument — a decision that was made deliberately and
written down is fine, even if it was the wrong one.
