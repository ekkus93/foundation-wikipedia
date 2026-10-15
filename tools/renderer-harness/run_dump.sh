#!/usr/bin/env bash
# Import a verified public-dump export (templates, modules, MediaWiki messages and
# a sample of articles) into the pinned MediaWiki image, render the sample with
# Parsoid and measure it against the same revisions on the live wiki.
# Usage: run_dump.sh OUT_DIR STAGED_MEMBER.xml.bz2 LANG_CODE LIVE_HOST
#   e.g. run_dump.sh out staging/angwiki-....xml.bz2 ang ang.wikipedia.org
set -euo pipefail

OUT="${1:?usage: run_dump.sh OUT_DIR STAGED_MEMBER LANG_CODE LIVE_HOST}"
MEMBER="${2:?missing staged member}"
LANG_CODE="${3:?missing wiki language code}"
LIVE_HOST="${4:?missing live host}"
# Pinned MediaWiki 1.44 release train includes bundled TemplateStyles (sanitized-css).
# Separate from the 1.43 synthetic fixture harness pin; never silently skip CSS pages.
IMAGE="${MW_IMAGE:-mediawiki@sha256:7bd97e0a6dd7d662e0cab93bb6a2e51b25c6a1d528f4e5ba88dc529dacf2224a}"
NAME="fwr-dump-$$"
PORT="${MW_PORT:-8081}"
BASE="http://127.0.0.1:${PORT}"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
ADMIN_PASS="$(openssl rand -hex 16)"

mkdir -p "$OUT"
cleanup() { docker logs "$NAME" >"$OUT/container.log" 2>&1 || true; docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT

python3 "$ROOT/scripts/filter_dump_for_render.py" "$MEMBER" "$OUT/filtered.xml" "$OUT/sample.json"

docker pull "$IMAGE"
docker run -d --name "$NAME" -p "127.0.0.1:${PORT}:80" "$IMAGE" >/dev/null
for _ in $(seq 1 60); do
  curl -s -o /dev/null "$BASE/" && break
  sleep 1
done
curl -s -o /dev/null "$BASE/"

docker exec -u root "$NAME" sh -c 'mkdir -p /var/www/data && chown www-data:www-data /var/www/data'
docker exec -u www-data "$NAME" php maintenance/run.php install \
  --dbtype=sqlite --dbpath=/var/www/data --dbname=dumpwiki \
  --server="$BASE" --scriptpath= --lang="$LANG_CODE" --pass="$ADMIN_PASS" DumpWiki Admin

# The dump includes sanitized-css pages; reject images missing TemplateStyles
# before importing any page, rather than silently losing template CSS.
if ! docker exec -u www-data "$NAME" test -f /var/www/html/extensions/TemplateStyles/extension.json; then
  echo "Renderer image lacks bundled TemplateStyles (sanitized-css content model)" >&2
  exit 1
fi

docker exec -u root -i "$NAME" sh -c 'cat >> /var/www/html/LocalSettings.php' <<'PHP'
foreach ( [ 'Cite', 'ParserFunctions', 'Scribunto', 'Math', 'Poem', 'ImageMap', 'TemplateStyles' ] as $fwrExt ) {
	if ( is_dir( "$IP/extensions/$fwrExt" ) ) {
		wfLoadExtension( $fwrExt );
	}
}
$wgScribuntoDefaultEngine = 'luastandalone';
$wgMathValidModes = [ 'native', 'source' ];
$wgDefaultUserOptions['math'] = 'native';
PHP

# Fail early if the configured wiki did not actually register the content model.
curl -fsS "$BASE/api.php?action=query&meta=siteinfo&siprop=general%7Cextensions&format=json" >"$OUT/siteinfo.json"
python3 - "$OUT/siteinfo.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    info = json.load(stream)
query = info.get("query", {})
generator = query.get("general", {}).get("generator", "")
names = {item.get("name") for item in query.get("extensions", [])}
if not generator.startswith("MediaWiki 1.44."):
    raise SystemExit(f"unexpected pinned renderer version: {generator!r}")
if "TemplateStyles" not in names:
    raise SystemExit("TemplateStyles is not registered; refusing incomplete dump import")
print(f"Renderer preflight: {generator}, TemplateStyles registered")
PY

START=$(date +%s)
docker exec -i -u www-data "$NAME" php maintenance/run.php importDump \
  --no-updates --report=500 --username-prefix=imported <"$OUT/filtered.xml" 2>&1 | tail -20
echo "import_seconds: $(( $(date +%s) - START ))" >"$OUT/timing.txt"

curl -fsS "$BASE/api.php?action=query&meta=siteinfo&siprop=general%7Cextensions%7Cstatistics&format=json" >"$OUT/siteinfo.json"
{
  echo "image: $IMAGE"
  docker image inspect --format '{{json .RepoDigests}}' "$IMAGE"
  docker exec "$NAME" php -v | head -1
} >"$OUT/environment.txt"

python3 "$ROOT/scripts/compare_rendered_html.py" \
  --sample "$OUT/sample.json" --local-base "$BASE" --live-host "$LIVE_HOST" --out "$OUT/fidelity.json"
