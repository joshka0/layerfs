#!/bin/bash
# Typechecks the model and runs TLC on every instance. The instance without a
# plant must hold every property; each plant must break the property it names.
# Exit 0 only when every expectation is met. Run from this folder after
#   sfw npm ci --no-audit --no-fund
set -u
cd "$(dirname "$0")"
quint=node_modules/.bin/quint
export JAVA_TOOL_OPTIONS=-XX:ActiveProcessorCount=2
failed=0
"$quint" typecheck sandbox_reopen.qnt || exit 1
# instance property kind expected(ok|violation)
while read -r plant property kind expected; do
  [ -z "$plant" ] && continue
  out=$(timeout 600 "$quint" verify "plant_$plant.qnt" --main "plant_$plant" \
    --init M::init --step M::stepOrDone --"$kind" "M::$property" --backend tlc 2>&1)
  if grep -q "No violation found" <<<"$out"; then got=ok; else got=violation; fi
  if [ "$got" = "$expected" ]; then mark=PASS; else mark=FAIL; failed=1; fi
  echo "$mark $plant $property: $got (expected $expected)"
done <<'TABLE'
none Safety invariant ok
none AllEnd temporal ok
no_kept NoOverlayLoss invariant violation
stale_restart OneDaemon invariant violation
no_guard OneDaemon invariant violation
leak_guard AllEnd temporal violation
TABLE
exit $failed
