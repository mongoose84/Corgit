#!/usr/bin/env bash
#
# Builds a throwaway folder of repositories to open twogit against — the root
# the README screenshots were taken on.
#
# The screenshots exist to show the left pane doing its job, which means the
# repos behind them need real variety: ahead, behind, dirty, staged, a feature
# branch, a repo with no upstream, and one with enough history to give the
# graph something to draw. Reproducing that by hand every time the UI changes
# is how screenshots go stale, so it lives here instead.
#
#   bash scripts/make-demo-root.sh [target]      # default: %TEMP%/twogit-demo
#
# Then open the printed folder in twogit. Nothing is written outside the
# target and its sibling `-origins` folder, and neither is inside this repo.
set -euo pipefail

DEMO="${1:-${TEMP:-/tmp}/twogit-demo}"
ORIGINS="$DEMO-origins"

rm -rf "$DEMO" "$ORIGINS"
mkdir -p "$DEMO" "$ORIGINS"

# The demo repos get their own git identity and no signing, so the script does
# not depend on — or disturb — whatever the machine's real config says.
export GIT_CONFIG_GLOBAL="$DEMO-gitconfig"
cat > "$GIT_CONFIG_GLOBAL" <<'EOF'
[user]
	name = Jeppe Kronborg
	email = jeppe@example.com
[init]
	defaultBranch = main
[commit]
	gpgsign = false
[core]
	autocrlf = false
EOF

DAYS_AGO=0

# Backdated so the graph's date column shows a spread of days rather than
# forty identical rows.
commit() {
	DAYS_AGO=$((DAYS_AGO - 1))
	local when
	when="$(date -d "$DAYS_AGO days ago 14:03" +'%Y-%m-%dT%H:%M:%S')"
	GIT_AUTHOR_DATE="$when" GIT_COMMITTER_DATE="$when" git commit -q -m "$1"
}

touch_file() {
	mkdir -p "$(dirname "$1")"
	printf '%s\n' "$2" >> "$1"
	git add "$1"
}

# Every repo gets a bare origin outside the root, so ahead/behind is real
# rather than simulated — the badges in the screenshots come from git.
new_repo() {
	git init -q --bare "$ORIGINS/$1.git"
	mkdir -p "$DEMO/$1"
	cd "$DEMO/$1"
	git init -q
	git remote add origin "$ORIGINS/$1.git"
}

# ── api-gateway — the repo the screenshots select, so it earns a history ────
new_repo api-gateway
touch_file README.md "# api-gateway"
commit "chore: scaffold service"
touch_file src/router.rs "pub fn route() {}"
commit "feat: add request router"
touch_file src/config.rs "pub struct Config;"
commit "feat: read config from environment"
touch_file src/health.rs "pub fn healthz() {}"
commit "feat: add health endpoint"
touch_file src/router.rs "// upstream selection"
commit "refactor: split upstream selection out of the router"

git switch -q -c feature/retry-policy
touch_file src/retry.rs "pub struct Retry;"
commit "feat: add retry policy"
touch_file src/retry.rs "// exponential backoff"
commit "feat: exponential backoff with jitter"
touch_file tests/retry.rs "// covers the backoff ceiling"
commit "test: cover the backoff ceiling"

git switch -q main
touch_file src/logging.rs "pub fn init() {}"
commit "feat: structured request logging"
touch_file README.md "Configuration lives in \`config.toml\`."
commit "docs: document configuration"

DAYS_AGO=$((DAYS_AGO - 1))
WHEN="$(date -d "$DAYS_AGO days ago 14:03" +'%Y-%m-%dT%H:%M:%S')"
GIT_AUTHOR_DATE="$WHEN" GIT_COMMITTER_DATE="$WHEN" \
	git merge -q --no-ff feature/retry-policy -m "merge: retry policy"

touch_file src/metrics.rs "pub fn counter() {}"
commit "feat: request counters"

# Unmerged, and branched off an older commit, so the graph has a second lane
# running alongside main rather than one straight line.
git switch -q -c feature/rate-limit HEAD~3
touch_file src/limit.rs "pub struct Bucket;"
commit "feat: token bucket rate limiter"
touch_file src/limit.rs "// per-tenant buckets"
commit "feat: per-tenant buckets"

git switch -q main
git push -q -u origin main
git push -q -u origin feature/rate-limit

# Unpushed → the row shows an ahead badge.
touch_file src/router.rs "// timeout handling"
commit "fix: honour upstream timeouts"
touch_file CHANGELOG.md "## Unreleased"
commit "docs: start a changelog"

# Dirty → the Uncommitted Changes node appears at the top of the graph, and
# the middle pane has something to stage.
printf '\n// TODO: connection pooling\n' >> src/router.rs
printf 'pub fn pool() {}\n' > src/pool.rs
printf 'scratch\n' > notes.txt
git add src/pool.rs

# ── The rest — enough variety that the left pane reads honestly ─────────────

simple_repo() {
	new_repo "$1"
	touch_file README.md "# $1"
	commit "chore: scaffold service"
	local i
	for ((i = 1; i <= $2; i++)); do
		touch_file "src/mod_$i.rs" "pub fn f$i() {}"
		commit "feat: step $i"
	done
	git push -q -u origin main
}

# Behind: origin moves on without us, and we fetch so the row knows it.
simple_repo auth-service 4
git switch -q -c tmp-origin
touch_file src/token.rs "pub fn verify() {}"
commit "feat: verify token audience"
touch_file src/token.rs "// clock skew"
commit "fix: allow 30s clock skew"
git push -q origin tmp-origin:main
git switch -q main
git branch -q -D tmp-origin
git fetch -q --prune

simple_repo billing-service 5

# Dirty, unstaged only.
simple_repo notification-worker 3
printf '\n// retry the webhook\n' >> src/mod_2.rs

# On a feature branch, ahead of its upstream.
simple_repo search-indexer 4
git switch -q -c feature/incremental-sync
touch_file src/sync.rs "pub fn sync() {}"
commit "feat: incremental sync"
git push -q -u origin feature/incremental-sync
touch_file src/sync.rs "// resume from cursor"
commit "feat: resume from a cursor"

simple_repo report-exporter 3

# Untracked files only — still a dirty dot, since the row has one state.
simple_repo tenant-admin-ui 6
printf 'draft\n' > TODO.md

simple_repo audit-log 4
simple_repo schedule-api 3

# No remote at all → the middle pane offers "Publish branch".
new_repo webhook-relay
touch_file README.md "# webhook-relay"
commit "chore: scaffold service"
touch_file src/relay.rs "pub fn relay() {}"
commit "feat: forward inbound hooks"
git remote remove origin

echo
echo "Demo root ready — open this folder in twogit:"
echo "  $DEMO"
