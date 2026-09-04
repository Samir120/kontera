# 07 — Release Engineering

**Status:** draft · **Owner:** Samir · **Last reviewed:** 2026-09-03

---

## 1. Versioning

Three version numbers, deliberately independent.

| Thing | Scheme | Notes |
|---|---|---|
| Crates | SemVer | Workspace crates released in lockstep |
| Event schema | Integer `schema_version` | Bumps only on breaking change |
| Config schema | Integer `schema_version` | Same |

Pre-1.0 the Rust API may change freely. **The event schema may not.** Hosts
persist events to disk, and those events are räkenskapsinformation subject to a
seven-year retention obligation. A v1 event log must still parse in 2033.

Practically: the deserialiser supports schema versions N and N−1. Dropping N−2
support is a major crate release with a migration note.

See [ADR-0005](adr/0005-event-schema-versioning.md).

## 2. Branching

`main` is always releasable. Short-lived feature branches, squash merge,
Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`).

Two extra scopes for this project:

- `schema:` — any change touching the event or config schema. Forces a
  deliberate pause.
- `sie:` — any change to the output writer. Requires re-running the acceptance
  test from [05 §5](05-testing-strategy.md#5-the-acceptance-test).

## 3. CI

GitHub Actions. Runners: `ubuntu-latest`, plus `macos-latest` for the CLI build.

**On every push and PR:**

```
fmt        cargo fmt --all --check
clippy     cargo clippy --all-targets --all-features -- -D warnings
test       cargo test --workspace
msrv       cargo +$MSRV check --workspace
deny       cargo deny check          # licences, bans, advisories
audit      cargo audit
purity     scripts/check-purity.sh   # no I/O, async, unsafe, clock in core
wasm       wasm-pack build crates/kontera-wasm && size report
```

**Nightly:** `cargo fuzz run` on all three targets, corpus committed on new finds.

**On tag:** the release workflow (§4).

### 3.1 `scripts/check-purity.sh`

The architectural guarantee from [03 §8](03-architecture.md#8-dependency-policy),
enforced mechanically rather than by discipline:

```bash
#!/usr/bin/env bash
set -euo pipefail
fail=0
check() {  # pattern, message
  if grep -rn "$1" crates/kontera-core/src crates/kontera-sie/src; then
    echo "PURITY VIOLATION: $2"; fail=1
  fi
}
check 'SystemTime::now\|Utc::now\|Local::now' 'clock read in a pure crate'
check 'std::fs\|std::net\|tokio\|async fn'    'I/O or async in a pure crate'
check 'unsafe '                               'unsafe in a pure crate'
grep -rn 'HashMap' crates/kontera-core/src/ledger.rs crates/kontera-sie/src \
  && { echo 'PURITY VIOLATION: HashMap in an output path'; fail=1; }
exit $fail
```

Crude, and it will produce the occasional false positive on a comment. That is an
acceptable trade for a check that runs in under a second and catches the exact
regressions that break determinism.

### 3.2 MSRV

Pinned in `Cargo.toml` (`rust-version`), tested in CI. Bumping the MSRV is a
minor release with a changelog note. Target: no newer than stable minus two
releases.

## 4. Release process

Tag-driven, using `release-plz` or `cargo-release`.

```
1. All CI green on main
2. Acceptance test re-run if anything under sie: changed
3. CHANGELOG.md updated (Keep a Changelog format)
4. cargo release --workspace <level>     # tags vX.Y.Z
5. Tag triggers:
     - cargo publish (core, sie, cli — in dependency order)
     - wasm-pack build --release && npm publish
     - GitHub Release with CLI binaries for linux-x86_64,
       linux-aarch64, macos-aarch64
```

`cargo publish --dry-run` runs on every PR that touches a manifest so publish
failures are found before tagging, not during.

### 4.1 The npm package

Package name tracks the crate name, resolved with the project-name decision.
**Not a company scope.** A scope like `@swadestack/kontera` signals that the
library belongs to one organisation's stack, which is the opposite of what an
embeddable library wants to communicate — and it makes the package awkward to
adopt for anyone outside that organisation. Prefer an unscoped name, or a scope
named after the project itself.

- Built from `kontera-wasm` via `wasm-pack --target bundler`
- TypeScript definitions **generated from the event schema**, not hand-written —
  hand-written types drift, and the drift shows up as a runtime error in someone
  else's product
- Version tracks the crate version exactly. Two version numbers for one artifact
  is a support burden nobody needs
- Size budget: 400 KB gzipped. Measured in CI, regression fails the build

## 5. Licensing

**Resolved 2026-09-04: MIT OR Apache-2.0.** `LICENSE-MIT` and `LICENSE-APACHE`
are in the repo root. The trade-off that was weighed:

| Option | Effect |
|---|---|
| MIT OR Apache-2.0 | Rust ecosystem default. Maximum adoption. Anyone, including a competitor, can build a SaaS on it |
| AGPL-3.0 | Network copyleft. Deters commercial SaaS forks. Also deters the commercial platform builders who are the target users |
| AGPL + commercial dual | Open core, sell exceptions. Real revenue path, real admin overhead |

For a library whose value is *being embedded*, AGPL is close to
self-defeating — the users are exactly the people whose lawyers will veto it.
Dual-licensing preserves optionality but only matters if there's a business.

**Recommendation:** MIT OR Apache-2.0 unless there's a concrete plan to sell it.
Relicensing later requires the agreement of every contributor, so if there's any
chance of a commercial path, decide before accepting the first outside PR.

Every published artifact carries the no-warranty disclaimer from
[01 §6](01-problem-and-scope.md#6-regulatory-posture) in `LICENSE`, `README`, and
the crate docs.

## 6. Supply chain and security

- `cargo-deny` with an explicit allowlist of licences; new dependencies need
  justification in the PR ([03 §8](03-architecture.md#8-dependency-policy))
- `cargo audit` on every build; advisories fail CI
- Dependabot for dependency PRs, grouped weekly
- `cargo publish` from CI using a scoped token in GitHub Secrets, never from a
  laptop
- Crate signing / provenance attestation once tooling stabilises

**Threat model note:** this library has no network surface and no persistence. Its
attack surface is deserialisation of a JSON event log supplied by the host. That
is why fuzzing the deserialiser is in the nightly job and why `post()` must never
panic — a panic in a WASM module takes the host's request down with it.

## 7. Documentation as a release artifact

- `cargo doc` published to docs.rs automatically
- `#![warn(missing_docs)]` on public items in `kontera-core`
- Doc examples compile and run as tests (`cargo test --doc`)
- The `docs/` set here is versioned with the code; a `schema:` change that
  doesn't update [04](04-public-api.md) fails review

## 8. Release checklist

```
[ ] CI green on main
[ ] Acceptance test re-run and recorded (if sie: touched)
[ ] All VERIFY markers resolved or explicitly deferred
[ ] Accountant review current (required for any release ≥ 0.1.0)
[ ] CHANGELOG.md updated
[ ] Event schema version unchanged, or migration note written
[ ] npm size budget met
[ ] cargo publish --dry-run clean
[ ] Disclaimer present in LICENSE, README, crate docs
```