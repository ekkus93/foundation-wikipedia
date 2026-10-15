#!/usr/bin/env bash
# Render the fixture wiki with an official MediaWiki image (bundled Parsoid,
# Cite, ParserFunctions, Scribunto/Lua) and check the Parsoid HTML.
# Usage: tools/renderer-harness/run.sh OUT_DIR   (requires docker, python3, curl)
set -euo pipefail

OUT="${1:?usage: run.sh OUT_DIR}"
# Digest of mediawiki:1.43 (MediaWiki 1.43.11, PHP 8.3.35) accepted in CI run 38799628018.
IMAGE="${MW_IMAGE:-mediawiki@sha256:76b3d47b50c4e4a78f82e814917421cb62c49b0c56b4a0f981cd74e2bddd10ac}"
NAME="fwr-harness-$$"
PORT="${MW_PORT:-8080}"
BASE="http://127.0.0.1:${PORT}"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
ADMIN_PASS="$(openssl rand -hex 16)"

mkdir -p "$OUT"
cleanup() { docker logs "$NAME" >"$OUT/container.log" 2>&1 || true; docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker pull "$IMAGE"
docker run -d --name "$NAME" -p "127.0.0.1:${PORT}:80" "$IMAGE" >/dev/null

for _ in $(seq 1 60); do
  curl -s -o /dev/null "$BASE/" && break
  sleep 1
done
curl -s -o /dev/null "$BASE/"

docker exec -u root "$NAME" sh -c 'mkdir -p /var/www/data && chown www-data:www-data /var/www/data'
docker exec -u www-data "$NAME" php maintenance/run.php install \
  --dbtype=sqlite --dbpath=/var/www/data --dbname=fixture \
  --server="$BASE" --scriptpath= --pass="$ADMIN_PASS" FixtureWiki Admin

docker exec -u root -i "$NAME" sh -c 'cat >> /var/www/html/LocalSettings.php' <<'PHP'
wfLoadExtension( 'Cite' );
wfLoadExtension( 'ParserFunctions' );
wfLoadExtension( 'Scribunto' );
$wgScribuntoDefaultEngine = 'luastandalone';
PHP

python3 - "$HERE/manifest.json" >"$OUT/pages.tsv" <<'PY'
import json, sys
for page in json.load(open(sys.argv[1], encoding="utf-8"))["pages"]:
    print(page["title"] + "\t" + page["file"])
PY
while IFS=$'\t' read -r title file; do
  docker exec -i -u www-data "$NAME" php maintenance/run.php edit \
    --user Admin --summary "fixture import" "$title" <"$HERE/$file"
done <"$OUT/pages.tsv"

curl -fsS "$BASE/api.php?action=query&meta=siteinfo&siprop=general%7Cextensions&format=json" >"$OUT/siteinfo.json"
{
  echo "image: $IMAGE"
  docker image inspect --format '{{json .RepoDigests}}' "$IMAGE"
  docker exec "$NAME" php -v | head -1
} >"$OUT/environment.txt"

python3 "$ROOT/scripts/check_renderer_fixtures.py" fetch \
  --base-url "$BASE" --manifest "$HERE/manifest.json" --out "$OUT/results.json"
python3 "$ROOT/scripts/check_renderer_fixtures.py" check \
  --manifest "$HERE/manifest.json" --results "$OUT/results.json" \
  --evidence-out "$OUT/evidence.json"
