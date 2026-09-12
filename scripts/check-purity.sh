# Mechanical enforcement of the purity boundary from ADR-0001 and docs/03 §8.
# Crude by design: it runs in under a second and catches exactly the
# regression that break determinism. An occasional false positive on a comment
# is an acceptable price. See docs/07 §3.1.

set -euo pipefail

CORE=crates/kontera-core/src
SIE=crates/kontera-sie/src

fail=0

# check <pattern> <message> <path...>
check() {
    local pattern="$1"; shift
    local message="$1"; shift
    local hits
if hits=$(grep -rnE "$pattern" "$@" 2>/dev/null); then
    echo "PURITY VIOLATION: $message"
    echo "$hits"
    echo
    fail=1
fi
}

check 'SystemTime::now|utc::now|Instant::now' \
    'clock read in a pure crate - dates are parameters (docs/08 §10)' \
    "$CORE" "$SIE"

check 'std::fs|std::net|tokio|async fn|\.await' \
    'I/O or async in a pure crate (ADR-0001)' \
    "$CORE" "$SIE"

check 'unsafe[[:space:]]' \
    'unsafe in a pure crate' \
    "$CORE" "$SIE"

check 'HashMap|HashSet' \
    'hash container in an output path -use BTreeMap/BTreeSet (docs/03 §4.2)' \
    "$CORE/ledger.rs" "$SIE"

if [ "$fail" -eq 0 ]; then
    echo "purity: ok"
fi

exit "$fail"
