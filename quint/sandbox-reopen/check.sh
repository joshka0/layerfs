#!/bin/bash
# Typechecks the model and runs TLC on every instance. The instance without a
# plant must hold every property; each plant must break the property it names.
# Each run checks one property, so a counterexample is for that property.
# A run counts only when the tool answers clearly:
#   ok         exit 0 and "No violation found"
#   violation  exit 1, "found a counterexample" and TLC's own violation line
#              for the kind checked (invariant or temporal)
# Any other result (a missing file, a parse error, a crash, a timeout) is an
# ERROR and fails the check. Exit 0 only when every expectation is met.
# Run from this folder after
#   sfw npm ci --no-audit --no-fund
# QUINT overrides the quint binary (to test this script's error handling).
set -u
cd "$(dirname "$0")"
quint=${QUINT:-node_modules/.bin/quint}
export JAVA_TOOL_OPTIONS=-XX:ActiveProcessorCount=2
mkdir -p results
failed=0
if ! "$quint" typecheck sandbox_reopen.qnt > results/typecheck.log 2>&1; then
  echo "ERROR typecheck failed (results/typecheck.log)"
  exit 1
fi
# instance property kind expected(ok|violation)
while read -r plant property kind expected; do
  [ -z "$plant" ] && continue
  log="results/$plant-$property.log"
  timeout 600 "$quint" verify "plant_$plant.qnt" --main "plant_$plant" \
    --init M::init --step M::stepOrDone --"$kind" "M::$property" --backend tlc > "$log" 2>&1
  code=$?
  case "$kind" in
    invariant) line="Error: Invariant .* is violated" ;;
    temporal) line="Error: Temporal propert(y|ies) .*was violated|Error: Temporal properties were violated" ;;
  esac
  if [ "$code" -eq 0 ] && grep -q "No violation found" "$log"; then
    got=ok
  elif [ "$code" -eq 1 ] && grep -q "found a counterexample" "$log" && grep -qE "$line" "$log"; then
    got=violation
  else
    got="error (exit $code, $log)"
  fi
  if [ "$got" = "$expected" ]; then mark=PASS; else mark=FAIL; failed=1; fi
  echo "$mark $plant $property: $got (expected $expected)"
done <<'TABLE'
none Safety invariant ok
none NoOverlayLoss invariant ok
none OneDaemon invariant ok
none OrphanNamed invariant ok
none ReopenedWhenDone invariant ok
none BootNeverDeletes invariant ok
none AllEnd temporal ok
none RevivedReported invariant ok
none StatusRefusesUnusable invariant ok
none StatusWaitsOutRestart invariant ok
none GuardReleased temporal ok
no_kept NoOverlayLoss invariant violation
no_running_check OneDaemon invariant ok
no_revive_check OneDaemon invariant ok
no_guard StatusWaitsOutRestart invariant violation
leak_guard GuardReleased temporal violation
boot_reclaims BootNeverDeletes invariant violation
keep_failed_daemon OneDaemon invariant ok
no_lock OneDaemon invariant violation
no_holder_name OrphanNamed invariant violation
no_revived_report RevivedReported invariant violation
status_no_wait StatusWaitsOutRestart invariant violation
no_stale_check StatusRefusesUnusable invariant violation
count_unattached ReopenedWhenDone invariant violation
TABLE
exit $failed
