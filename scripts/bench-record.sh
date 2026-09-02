#!/usr/bin/env bash
# Record a benchmark run, store it, and compare it to the previous run.
#
# Runs both benchmark suites (view-model + core), merges their JSON output into
# one result file under benchmarks/results/<git-sha>-<timestamp>.json, and prints
# a line-by-line diff against the most recent previous result (if any).
#
# The individual result files are git-ignored (ephemeral + noisy); the comparable
# artifact is the trend this script prints. Metrics are stable across releases.
#
# Environment:
#   KAPTEIN_BENCH_GIT_SHA   commit the run is recorded against (defaults to `git rev-parse HEAD`)
#   KAPTEIN_BENCH_HOST      optional host/runner label written into the result file

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

GIT_SHA="${KAPTEIN_BENCH_GIT_SHA:-$(git rev-parse HEAD)}"
HOST="${KAPTEIN_BENCH_HOST:-$(hostname 2>/dev/null || echo unknown)}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="$(mktemp -d)"
RESULTS_DIR="$REPO_ROOT/benchmarks/results"
RESULT_FILE="$RESULTS_DIR/${GIT_SHA:0:12}-${STAMP}.json"

trap 'rm -rf "$OUT_DIR"' EXIT

echo "==> Running view-model bench (query) …"
KAPTEIN_BENCH_OUT="$OUT_DIR" KAPTEIN_BENCH_GIT_SHA="$GIT_SHA" \
  cargo bench -p kaptein-viewmodel --bench query --locked

echo "==> Running core bench (core_paths) …"
KAPTEIN_BENCH_OUT="$OUT_DIR" KAPTEIN_BENCH_GIT_SHA="$GIT_SHA" \
  cargo bench -p kaptein-core --bench core_paths --locked

# Merge the two suites' metrics into one flat result.
python3 - "$OUT_DIR" "$GIT_SHA" "$HOST" "$RESULT_FILE" <<'PY'
import json, sys, pathlib

out_dir, git_sha, host, result_file = sys.argv[1:5]
metrics = {}
for suite in ("viewmodel", "core"):
    p = pathlib.Path(out_dir) / f"{suite}.json"
    if not p.exists():
        print(f"warning: missing {suite}.json — skipping", file=sys.stderr)
        continue
    d = json.loads(p.read_text())
    assert d.get("schema") == "kaptein-benchmark/v1", f"{suite}.json bad schema"
    metrics.update(d["metrics"])

result = {
    "schema": "kaptein-benchmark/v1",
    "suite": "kaptein",
    "git_sha": git_sha,
    "host": host,
    "metrics": metrics,
}

pathlib.Path(result_file).parent.mkdir(parents=True, exist_ok=True)
pathlib.Path(result_file).write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
print(f"==> Wrote {result_file}")
PY

# Diff against the previous run, if one exists.
PREV="$(ls -1 "$RESULTS_DIR"/*.json 2>/dev/null | grep -v "$RESULT_FILE" | tail -1 || true)"
if [[ -n "$PREV" ]]; then
  echo "==> Comparing to previous run: $(basename "$PREV")"
  python3 - "$PREV" "$RESULT_FILE" <<'PY'
import json, sys
prev = json.load(open(sys.argv[1]))["metrics"]
cur = json.load(open(sys.argv[2]))["metrics"]
keys = sorted(set(prev) | set(cur))
width = max(len(k) for k in keys)
print(f"{'metric':<{width}}  {'previous':>12}  {'current':>12}  {'Δ':>12}")
for k in keys:
    p, c = prev.get(k), cur.get(k)
    if p is None or c is None:
        print(f"{k:<{width}}  {str(p):>12}  {str(c):>12}  {'new/removed':>12}")
        continue
    delta = c - p
    sign = "+" if delta > 0 else ""
    print(f"{k:<{width}}  {p:>12.3f}  {c:>12.3f}  {sign}{delta:.3f}")
PY
fi
