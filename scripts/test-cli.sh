#!/usr/bin/env bash
set -euo pipefail
cli="$(pwd)/target/debug/diff-ex-cli"
d=$(mktemp -d);trap 'rm -rf "$d"' EXIT
printf 'one\n' > "$d/a.txt";printf 'two\n' > "$d/b.txt"
"$cli" compare text "$d/a.txt" "$d/a.txt" --output "$d/equal.json"
set +e
"$cli" compare text "$d/a.txt" "$d/b.txt" --format html --output "$d/different.html"
code=$?
set -e
test "$code" = 1
python3 - "$d" <<'PY'
import json,pathlib,sys
d=pathlib.Path(sys.argv[1]); r=json.loads((d/'equal.json').read_text())
assert r['complete'] and r['results'][0]['status']=='equal'
assert '<html' in (d/'different.html').read_text()
(d/'manifest.json').write_text(json.dumps({'version':1,'tasks':[{'id':'ok','kind':'text','left':str(d/'a.txt'),'right':str(d/'a.txt')},{'id':'error','kind':'text','left':str(d/'a.txt'),'right':str(d/'missing.txt')}]}))
PY
set +e
"$cli" batch "$d/manifest.json" --output "$d/batch.json"
code=$?
"$cli" compare text "$d/a.txt" "$d/a.txt" --output "$d/a.txt"
overwrite=$?
set -e
test "$code" = 2
test "$overwrite" = 2
test "$(cat "$d/a.txt")" = one
python3 - "$d/batch.json" <<'PY'
import json,sys
r=json.load(open(sys.argv[1]));assert r['complete'];assert [x['status'] for x in r['results']]==['equal','error']
PY
