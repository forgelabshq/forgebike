#!/usr/bin/env bash
# =============================================================================
# scripts/test.sh — Forgebike development environment + full test suite
#
# What this does:
#   1. Starts docker compose (PostgreSQL + Redis) if not already running
#   2. Starts the server (with hot reload via cargo-watch if installed,
#      otherwise plain cargo run)
#   3. Waits for the server to be ready (handles first-time compilation)
#   4. Runs every implemented test with coloured pass / fail output
#   5. Prints a summary, then leaves the server running for development
#
# Usage:
#   ./scripts/test.sh                  # start everything, run tests, keep running
#   TEARDOWN=true ./scripts/test.sh    # same but stop everything when done
#   BASE_URL=http://... ./scripts/test.sh  # test against a different host
#
# Prerequisites: bash, curl, python3, docker, cargo
# Optional:      cargo-watch  (cargo install cargo-watch)
# =============================================================================

set -euo pipefail

# ── Tunables (override via environment) ───────────────────────────────────────
BASE_URL="${BASE_URL:-http://localhost:8080}"
MAX_WAIT="${MAX_WAIT:-180}"          # seconds to wait for server to start
LOG_FILE="${LOG_FILE:-/tmp/forgebike-server.log}"
TEARDOWN="${TEARDOWN:-false}"

# Use a generous rate-limit burst so the test suite never hits 429.
# Tighten APP__RATE_LIMIT__BURST_SIZE in production.
export APP__RATE_LIMIT__BURST_SIZE="${APP__RATE_LIMIT__BURST_SIZE:-200}"
export APP__RATE_LIMIT__PER_SECOND="${APP__RATE_LIMIT__PER_SECOND:-100}"

# ── ANSI colours ──────────────────────────────────────────────────────────────
if [ -t 1 ]; then          # only colourise if stdout is a terminal
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    BLUE='\033[1;34m'
    CYAN='\033[0;36m'
    BOLD='\033[1m'
    DIM='\033[2m'
    NC='\033[0m'
else
    RED='' GREEN='' YELLOW='' BLUE='' CYAN='' BOLD='' DIM='' NC=''
fi

# ── Counters & state ──────────────────────────────────────────────────────────
PASS=0
FAIL=0
SERVER_PID=""
WE_STARTED_SERVER=false
WE_STARTED_DOCKER=false

# ── Cleanup on exit ───────────────────────────────────────────────────────────
cleanup() {
    # Restore cursor if we were writing progress dots
    echo ""
    if [ "${TEARDOWN}" = "true" ]; then
        echo -e "${YELLOW}Tearing down…${NC}"
        if [ -n "${SERVER_PID}" ]; then
            kill "${SERVER_PID}" 2>/dev/null || true
            echo "  Server stopped (was PID ${SERVER_PID})"
        fi
        if [ "${WE_STARTED_DOCKER}" = "true" ]; then
            docker compose down --remove-orphans 2>/dev/null
            echo "  Infrastructure stopped"
        fi
    else
        if [ "${WE_STARTED_SERVER}" = "true" ] && [ -n "${SERVER_PID}" ]; then
            echo -e "${CYAN}${BOLD}Server is still running — hot reload is active.${NC}"
            echo -e "  ${DIM}Logs : tail -f ${LOG_FILE}${NC}"
            echo -e "  ${DIM}Stop : kill ${SERVER_PID}${NC}"
            echo -e "  ${DIM}Re-run tests: ./scripts/test.sh${NC}"
            echo -e "  ${DIM}Full teardown: TEARDOWN=true ./scripts/test.sh${NC}"
        fi
    fi
}

trap cleanup EXIT

# ── Output helpers ─────────────────────────────────────────────────────────────
section() {
    echo -e ""
    echo -e "${BOLD}${BLUE}┌──────────────────────────────────────────────────────┐${NC}"
    printf  "${BOLD}${BLUE}│  %-52s│${NC}\n" "$1"
    echo -e "${BOLD}${BLUE}└──────────────────────────────────────────────────────┘${NC}"
}
subsection() { echo -e "\n  ${BOLD}$1${NC}"; }
info()       { echo -e "  ${CYAN}›${NC} $*"; }
ok()         { echo -e "  ${GREEN}✓${NC} $*"; }
warn()       { echo -e "  ${YELLOW}⚠${NC} $*"; }
die()        { echo -e "\n  ${RED}✗ FATAL: $*${NC}" >&2; exit 1; }

# ── HTTP helpers ───────────────────────────────────────────────────────────────
# Every call appends \n<http_status_code> to the response body.
# Use body() / status() to split them apart.

_GET() {
    curl -s --max-time 10 -w "\n%{http_code}" "$@"
}
_POST() {
    curl -s --max-time 10 -w "\n%{http_code}" \
         -X POST -H "Content-Type: application/json" "$@"
}

body()   { printf '%s' "$1" | head -n -1; }
status() { printf '%s' "$1" | tail  -n  1; }

# field <json> <python_key_expression>   e.g.  field "$B" "['role']"
field() {
    printf '%s' "$1" \
      | python3 -c "import sys,json; d=json.load(sys.stdin); print(d$2)" 2>/dev/null \
      || echo "__missing__"
}

# has_field <json> <key>  →  "yes" | "no"
has_field() {
    printf '%s' "$1" \
      | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    print('yes' if d.get('$2') else 'no')
except Exception:
    print('no')
" 2>/dev/null || echo "no"
}

# ── Assertion helpers ──────────────────────────────────────────────────────────
_pass() { echo -e "    ${GREEN}✓${NC}  $1"; PASS=$((PASS+1)); }
_fail() {
    echo -e "    ${RED}✗${NC}  $1"
    echo -e "       ${DIM}↳ $2${NC}"
    FAIL=$((FAIL+1))
}

assert_status() {          # assert_status "label" expected actual
    local label="$1" exp="$2" got="$3"
    if [ "$got" = "$exp" ]; then _pass "$label"
    else _fail "$label" "expected HTTP $exp, got HTTP $got"; fi
}

assert_eq() {              # assert_eq "label" expected actual
    local label="$1" exp="$2" got="$3"
    if [ "$got" = "$exp" ]; then _pass "$label"
    else _fail "$label" "expected '$exp', got '$got'"; fi
}

assert_present() {         # assert_present "label" yes|no
    local label="$1" result="$2"
    if [ "$result" = "yes" ]; then _pass "$label"
    else _fail "$label" "field missing or empty in response"; fi
}

assert_not_eq() {          # assert_not_eq "label" unexpected actual
    local label="$1" bad="$2" got="$3"
    if [ "$got" != "$bad" ]; then _pass "$label"
    else _fail "$label" "value should not equal '$bad'"; fi
}

# ── Infrastructure ─────────────────────────────────────────────────────────────
start_infrastructure() {
    section "Infrastructure"

    # Check whether postgres is already reachable on 5432 (native) or 5435 (docker)
    if PGPASSWORD=password psql -h 127.0.0.1 -p 5432 -U postgres \
           -d forgebike -c '\q' > /dev/null 2>&1; then
        info "PostgreSQL is reachable on port 5432 (native instance)"
    elif docker compose ps --status=running 2>/dev/null | grep -q "postgres"; then
        info "PostgreSQL is reachable via docker compose"
    else
        info "Starting docker compose services…"
        docker compose up -d
        WE_STARTED_DOCKER=true

        info "Waiting for PostgreSQL to be healthy…"
        local i=0
        until docker compose exec -T postgres \
              pg_isready -U postgres > /dev/null 2>&1; do
            sleep 1; i=$((i+1))
            [ $i -ge 30 ] && die "PostgreSQL did not become healthy in 30s"
        done
        ok "PostgreSQL ready"

        info "Waiting for Redis to be healthy…"
        until docker compose exec -T redis \
              redis-cli ping > /dev/null 2>&1; do sleep 1; done
        ok "Redis ready"
    fi
}

# ── Server ─────────────────────────────────────────────────────────────────────
start_server() {
    section "Server"

    # Already running? Nothing to do.
    if curl -s --max-time 2 "${BASE_URL}/health" > /dev/null 2>&1; then
        info "Server already running at ${BASE_URL}"
        return
    fi

    if cargo watch --version > /dev/null 2>&1; then
        info "Starting server with hot reload (cargo watch)…"
        cargo watch -q -x 'run --bin forgebike' \
            >> "${LOG_FILE}" 2>&1 &
    else
        warn "cargo-watch not installed — using cargo run (no hot reload)"
        warn "Enable hot reload: cargo install cargo-watch"
        cargo run --bin forgebike \
            >> "${LOG_FILE}" 2>&1 &
    fi

    SERVER_PID=$!
    WE_STARTED_SERVER=true
    # Detach so the process survives this script's exit.
    disown "${SERVER_PID}" 2>/dev/null || true

    info "Compiling (PID ${SERVER_PID}) — first run can take ~60 s"
    info "Logs streaming to: ${LOG_FILE}"
    echo ""

    local waited=0
    while ! curl -s --max-time 2 "${BASE_URL}/health" > /dev/null 2>&1; do
        # Show last non-empty log line as a live progress indicator.
        local last
        last=$(grep -v '^[[:space:]]*$' "${LOG_FILE}" 2>/dev/null \
               | tail -1 \
               | cut -c1-72 \
               || echo "waiting…")
        printf "  \r  ${DIM}%-72s${NC}" "${last}"
        sleep 1
        waited=$((waited+1))
        if [ "${waited}" -ge "${MAX_WAIT}" ]; then
            echo ""
            echo -e "${RED}Server did not start within ${MAX_WAIT}s.${NC}"
            echo "Last 20 log lines:"
            tail -20 "${LOG_FILE}" 2>/dev/null || true
            die "Aborting — fix compilation errors and re-run"
        fi
    done

    printf "\r%76s\r" ""   # clear the progress line
    ok "Server is ready at ${BASE_URL}"
}

# =============================================================================
# ── Test suite ────────────────────────────────────────────────────────────────
# =============================================================================

# ── Phase 0 — Foundations ─────────────────────────────────────────────────────
test_phase_0() {
    section "Phase 0 — Foundations"

    subsection "GET /health"
    local R B
    R=$(_GET "${BASE_URL}/health")
    B=$(body "$R")

    assert_status  "returns 200"                     "200"  "$(status "$R")"
    assert_eq      "status field is 'ok'"            "ok"   "$(field "$B" "['status']")"
    assert_eq      "database component is ok"        "ok"   "$(field "$B" "['components']['database']")"
    assert_eq      "redis component is ok"           "ok"   "$(field "$B" "['components']['redis']")"
}

# ── Phase 1 — Auth ────────────────────────────────────────────────────────────
test_phase_1() {
    section "Phase 1 — Auth & Multi-Tenancy"

    # ── Registration — happy path ─────────────────────────────────────────────
    subsection "POST /api/v1/auth/register — valid payload"
    local R B
    R=$(_POST "${BASE_URL}/api/v1/auth/register" \
        -d '{"business_name":"Test Bistro","email":"owner@testbistro.dev","password":"supersecret99"}')
    B=$(body "$R")

    assert_status  "returns 201 Created"             "201"    "$(status "$R")"
    assert_present "access_token is present"                  "$(has_field "$B" "access_token")"
    assert_present "refresh_token is present"                 "$(has_field "$B" "refresh_token")"
    assert_eq      "token_type is 'Bearer'"          "Bearer" "$(field "$B" "['token_type']")"
    assert_eq      "expires_in is 900 (15 min)"      "900"    "$(field "$B" "['expires_in']")"

    # Capture tokens for subsequent tests
    local ACCESS REFRESH
    ACCESS=$(field "$B" "['access_token']")
    REFRESH=$(field "$B" "['refresh_token']")

    # ── Registration — validation errors ─────────────────────────────────────
    subsection "POST /api/v1/auth/register — validation"

    R=$(_POST "${BASE_URL}/api/v1/auth/register" \
        -d '{"business_name":"X","email":"not-an-email","password":"validpass99"}')
    assert_status  "invalid email → 422"             "422" "$(status "$R")"

    R=$(_POST "${BASE_URL}/api/v1/auth/register" \
        -d '{"business_name":"X","email":"valid@email.com","password":"short"}')
    assert_status  "password < 8 chars → 422"        "422" "$(status "$R")"

    R=$(_POST "${BASE_URL}/api/v1/auth/register" \
        -d '{"business_name":"","email":"valid@email.com","password":"validpass99"}')
    assert_status  "empty business_name → 422"       "422" "$(status "$R")"

    R=$(_POST "${BASE_URL}/api/v1/auth/register" \
        -d '{"email":"valid@email.com","password":"validpass99"}')
    assert_status  "missing business_name field → 422" "422" "$(status "$R")"

    R=$(_POST "${BASE_URL}/api/v1/auth/register" \
        -d '{"business_name":"X","email":"valid@email.com"}')
    assert_status  "missing password field → 422"    "422" "$(status "$R")"

    # ── GET /me — authentication checks ──────────────────────────────────────
    subsection "GET /api/v1/auth/me — authentication"

    R=$(_GET "${BASE_URL}/api/v1/auth/me" -H "Authorization: Bearer ${ACCESS}")
    B=$(body "$R")
    assert_status  "valid JWT → 200"                 "200"   "$(status "$R")"
    assert_present "user_id is present"                      "$(has_field "$B" "user_id")"
    assert_present "tenant_id is present"                    "$(has_field "$B" "tenant_id")"
    assert_eq      "role is 'owner'"                 "owner" "$(field "$B" "['role']")"

    R=$(_GET "${BASE_URL}/api/v1/auth/me")
    assert_status  "no token → 401"                  "401" "$(status "$R")"

    R=$(_GET "${BASE_URL}/api/v1/auth/me" -H "Authorization: Bearer this.is.fake")
    assert_status  "malformed JWT → 401"             "401" "$(status "$R")"

    R=$(_GET "${BASE_URL}/api/v1/auth/me" -H "Authorization: Token ${ACCESS}")
    assert_status  "wrong auth scheme (Token ≠ Bearer) → 401" "401" "$(status "$R")"

    R=$(_GET "${BASE_URL}/api/v1/auth/me" -H "Authorization: Bearer ")
    assert_status  "empty bearer value → 401"        "401" "$(status "$R")"

    # ── Login — happy path ────────────────────────────────────────────────────
    subsection "POST /api/v1/auth/login — valid credentials"

    R=$(_POST "${BASE_URL}/api/v1/auth/login" \
        -d '{"email":"owner@testbistro.dev","password":"supersecret99"}')
    B=$(body "$R")
    assert_status  "correct credentials → 200"       "200"    "$(status "$R")"
    assert_present "access_token is present"                  "$(has_field "$B" "access_token")"
    assert_present "refresh_token is present"                 "$(has_field "$B" "refresh_token")"
    assert_eq      "token_type is 'Bearer'"          "Bearer" "$(field "$B" "['token_type']")"

    # ── Login — rejection ─────────────────────────────────────────────────────
    subsection "POST /api/v1/auth/login — rejection"

    R=$(_POST "${BASE_URL}/api/v1/auth/login" \
        -d '{"email":"owner@testbistro.dev","password":"wrongpassword"}')
    B=$(body "$R")
    assert_status  "wrong password → 401"            "401" "$(status "$R")"
    assert_eq      "generic error — no field hint" \
                   "Invalid email or password"            "$(field "$B" "['error']")"

    R=$(_POST "${BASE_URL}/api/v1/auth/login" \
        -d '{"email":"ghost@nobody.test","password":"anypassword"}')
    B=$(body "$R")
    assert_status  "unknown email → 401"             "401" "$(status "$R")"
    assert_eq      "same generic error (no user enumeration)" \
                   "Invalid email or password"            "$(field "$B" "['error']")"

    R=$(_POST "${BASE_URL}/api/v1/auth/login" \
        -d '{"email":"not-an-email","password":"validpass99"}')
    assert_status  "invalid email format → 422"      "422" "$(status "$R")"

    # ── Token refresh ─────────────────────────────────────────────────────────
    subsection "POST /api/v1/auth/refresh"

    R=$(_POST "${BASE_URL}/api/v1/auth/refresh" \
        -d "{\"refresh_token\":\"${REFRESH}\"}")
    B=$(body "$R")
    assert_status  "valid refresh token → 200"       "200" "$(status "$R")"
    assert_present "new access_token present"                "$(has_field "$B" "access_token")"
    assert_present "new refresh_token present"               "$(has_field "$B" "refresh_token")"

    local NEW_ACCESS NEW_REFRESH
    NEW_ACCESS=$(field  "$B" "['access_token']")
    NEW_REFRESH=$(field "$B" "['refresh_token']")

    # Tokens must actually be different (rotation)
    assert_not_eq  "new access_token differs from original"  \
                   "${ACCESS}"      "${NEW_ACCESS}"
    assert_not_eq  "new refresh_token differs from original" \
                   "${REFRESH}"     "${NEW_REFRESH}"

    # Original refresh token is now dead
    R=$(_POST "${BASE_URL}/api/v1/auth/refresh" \
        -d "{\"refresh_token\":\"${REFRESH}\"}")
    assert_status  "rotated (old) refresh token → 401"        "401" "$(status "$R")"

    # New access token is immediately usable
    R=$(_GET "${BASE_URL}/api/v1/auth/me" -H "Authorization: Bearer ${NEW_ACCESS}")
    assert_status  "new access token works on /me → 200"      "200" "$(status "$R")"

    R=$(_POST "${BASE_URL}/api/v1/auth/refresh" \
        -d '{"refresh_token":"not-a-real-uuid-token"}')
    assert_status  "garbage refresh token → 401"              "401" "$(status "$R")"

    R=$(_POST "${BASE_URL}/api/v1/auth/refresh" \
        -d '{"refresh_token":""}')
    assert_status  "empty refresh token → 422"                "422" "$(status "$R")"

    # ── Logout ────────────────────────────────────────────────────────────────
    subsection "POST /api/v1/auth/logout"

    R=$(_POST "${BASE_URL}/api/v1/auth/logout" \
        -d "{\"refresh_token\":\"${NEW_REFRESH}\"}")
    assert_status  "valid token → 204 No Content"             "204" "$(status "$R")"

    # Refresh after logout is blocked
    R=$(_POST "${BASE_URL}/api/v1/auth/refresh" \
        -d "{\"refresh_token\":\"${NEW_REFRESH}\"}")
    assert_status  "refresh after logout → 401"               "401" "$(status "$R")"

    # Logout is idempotent (already-revoked token)
    R=$(_POST "${BASE_URL}/api/v1/auth/logout" \
        -d "{\"refresh_token\":\"${NEW_REFRESH}\"}")
    assert_status  "double logout is idempotent → 204"        "204" "$(status "$R")"

    # Access token still valid until natural expiry (JWT is stateless)
    R=$(_GET "${BASE_URL}/api/v1/auth/me" -H "Authorization: Bearer ${NEW_ACCESS}")
    assert_status  "/me still works after logout (JWT valid until exp) → 200" \
                   "200" "$(status "$R")"
}

# ── Summary ────────────────────────────────────────────────────────────────────
print_summary() {
    local total=$((PASS + FAIL))
    echo ""
    echo -e "${BOLD}${BLUE}┌──────────────────────────────────────────────────────┐${NC}"
    echo -e "${BOLD}${BLUE}│  Results                                             │${NC}"
    echo -e "${BOLD}${BLUE}├──────────────────────────────────────────────────────┤${NC}"
    printf  "${BOLD}${BLUE}│${NC}  ${GREEN}%-6s passed${NC}  ${RED}%-6s failed${NC}  ${DIM}%-6s total${NC}          ${BOLD}${BLUE}│${NC}\n" \
            "${PASS}" "${FAIL}" "${total}"
    echo -e "${BOLD}${BLUE}└──────────────────────────────────────────────────────┘${NC}"

    if [ "${FAIL}" -eq 0 ]; then
        echo -e "\n  ${GREEN}${BOLD}All ${total} tests passed ✓${NC}"
    else
        echo -e "\n  ${RED}${BOLD}${FAIL} / ${total} tests failed ✗${NC}"
    fi
    echo ""
}

# ── Phase 2 tests ──────────────────────────────────────────────────────────────────
test_phase_2() {
    section "Phase 2 — Restaurants & Menus"

    # First we need a valid token; register a fresh account for this phase.
    local REG_R
    REG_R=$(_POST "${BASE_URL}/api/v1/auth/register" \
        -d '{"business_name":"Phase2 Corp","email":"p2@test.dev","password":"password99"}')
    local TOKEN
    TOKEN=$(field "$(body "${REG_R}")" "['access_token']")

    AUTH="-H \"Authorization: Bearer ${TOKEN}\""

    # ── Unauthenticated access ────────────────────────────────────────────────────
    subsection "GET /api/v1/restaurants — no auth"
    local R
    R=$(_GET "${BASE_URL}/api/v1/restaurants")
    assert_status "no token → 401" "401" "$(status "${R}")"

    R=$(_POST "${BASE_URL}/api/v1/restaurants" \
        -d '{"name":"Test"}')
    assert_status "POST without token → 401" "401" "$(status "${R}")"

    # ── Restaurant CRUD ───────────────────────────────────────────────────────────
    subsection "POST /api/v1/restaurants — create"
    R=$(_POST "${BASE_URL}/api/v1/restaurants" \
        -H "Authorization: Bearer ${TOKEN}" \
        -d '{
          "name": "The Golden Fork",
          "description": "Fine dining in the city centre",
          "cuisine_type": "Modern European",
          "address": "1 Harbour Lane, London",
          "phone": "+44 20 7946 0001"
        }')
    local RB; RB=$(body "${R}")
    assert_status "create restaurant → 201"           "201"                     "$(status "${R}")"
    assert_present "  id present"                                               "$(has_field "${RB}" "id")"
    assert_eq      "  name correct"          "The Golden Fork"                  "$(field "${RB}" "['name']")"
    assert_eq      "  cuisine_type correct"  "Modern European"                  "$(field "${RB}" "['cuisine_type']")"

    local REST_ID; REST_ID=$(field "${RB}" "['id']")

    subsection "POST /api/v1/restaurants — validation"
    R=$(_POST "${BASE_URL}/api/v1/restaurants" \
        -H "Authorization: Bearer ${TOKEN}" \
        -d '{"name":""}')
    assert_status "empty name → 422"  "422" "$(status "${R}")"

    R=$(_POST "${BASE_URL}/api/v1/restaurants" \
        -H "Authorization: Bearer ${TOKEN}" \
        -d '{}')
    assert_status "missing name → 422" "422" "$(status "${R}")"

    subsection "GET /api/v1/restaurants/:id — fetch"
    R=$(_GET "${BASE_URL}/api/v1/restaurants/${REST_ID}" \
        -H "Authorization: Bearer ${TOKEN}")
    RB=$(body "${R}")
    assert_status "get by id → 200"           "200"             "$(status "${R}")"
    assert_eq      "  id matches"              "${REST_ID}"     "$(field "${RB}" "['id']")"
    assert_eq      "  name matches"            "The Golden Fork" "$(field "${RB}" "['name']")"

    R=$(_GET "${BASE_URL}/api/v1/restaurants/00000000-0000-0000-0000-000000000000" \
        -H "Authorization: Bearer ${TOKEN}")
    assert_status "unknown id → 404" "404" "$(status "${R}")"

    subsection "PATCH /api/v1/restaurants/:id — update"
    R=$(_POST "${BASE_URL}/api/v1/restaurants/${REST_ID}" \
        -X PATCH \
        -H "Authorization: Bearer ${TOKEN}" \
        -d '{"name":"The Silver Spoon","cuisine_type":"Italian"}')
    RB=$(body "${R}")
    assert_status "update → 200"            "200"             "$(status "${R}")"
    assert_eq      "  name updated"          "The Silver Spoon" "$(field "${RB}" "['name']")"
    assert_eq      "  cuisine updated"       "Italian"          "$(field "${RB}" "['cuisine_type']")"
    assert_eq      "  address preserved"     "1 Harbour Lane, London" "$(field "${RB}" "['address']")"

    subsection "GET /api/v1/restaurants — list"
    # Create a second restaurant for pagination testing.
    _POST "${BASE_URL}/api/v1/restaurants" \
        -H "Authorization: Bearer ${TOKEN}" \
        -d '{"name":"Cafe Bleu","cuisine_type":"French"}' > /dev/null

    R=$(_GET "${BASE_URL}/api/v1/restaurants" \
        -H "Authorization: Bearer ${TOKEN}")
    RB=$(body "${R}")
    assert_status "list → 200"               "200" "$(status "${R}")"
    assert_present "  items array present"          "$(has_field "${RB}" "items")"

    # List with limit=1 to force pagination.
    R=$(_GET "${BASE_URL}/api/v1/restaurants?limit=1" \
        -H "Authorization: Bearer ${TOKEN}")
    RB=$(body "${R}")
    assert_status  "list?limit=1 → 200"       "200" "$(status "${R}")"
    assert_present "  next_cursor present"           "$(has_field "${RB}" "next_cursor")"

    local CURSOR; CURSOR=$(field "${RB}" "['next_cursor']")
    R=$(_GET "${BASE_URL}/api/v1/restaurants?limit=1&cursor=${CURSOR}" \
        -H "Authorization: Bearer ${TOKEN}")
    RB=$(body "${R}")
    assert_status "list page 2 → 200"         "200" "$(status "${R}")"
    assert_present "  second page has items"         "$(has_field "${RB}" "items")"

    # ── Menu items ─────────────────────────────────────────────────────────────────
    subsection "POST /api/v1/restaurants/:id/menu — create item"
    R=$(_POST "${BASE_URL}/api/v1/restaurants/${REST_ID}/menu" \
        -H "Authorization: Bearer ${TOKEN}" \
        -d '{
          "name": "Beef Bourguignon",
          "description": "Classic French braise",
          "price_cents": 2850,
          "category": "Mains"
        }')
    RB=$(body "${R}")
    assert_status "create item → 201"          "201"              "$(status "${R}")"
    assert_eq      "  name correct"            "Beef Bourguignon" "$(field "${RB}" "['name']")"
    assert_eq      "  price_cents correct"     "2850"             "$(field "${RB}" "['price_cents']")"
    assert_eq      "  is_available default"    "True"             "$(field "${RB}" "['is_available']")"

    local ITEM_ID; ITEM_ID=$(field "${RB}" "['id']")

    subsection "POST /api/v1/restaurants/:id/menu — validation"
    R=$(_POST "${BASE_URL}/api/v1/restaurants/${REST_ID}/menu" \
        -H "Authorization: Bearer ${TOKEN}" \
        -d '{"name":""}')
    assert_status "empty name → 422" "422" "$(status "${R}")"

    R=$(_POST "${BASE_URL}/api/v1/restaurants/${REST_ID}/menu" \
        -H "Authorization: Bearer ${TOKEN}" \
        -d '{"name":"Soup","price_cents":-1}')
    assert_status "negative price → 422" "422" "$(status "${R}")"

    subsection "GET /api/v1/restaurants/:id/menu — list"
    R=$(_GET "${BASE_URL}/api/v1/restaurants/${REST_ID}/menu" \
        -H "Authorization: Bearer ${TOKEN}")
    RB=$(body "${R}")
    assert_status "list menu → 200"         "200" "$(status "${R}")"
    assert_present "  items array present"        "$(has_field "${RB}" "items")"

    subsection "PATCH /api/v1/restaurants/:id/menu/:item_id — update"
    R=$(_POST "${BASE_URL}/api/v1/restaurants/${REST_ID}/menu/${ITEM_ID}" \
        -X PATCH \
        -H "Authorization: Bearer ${TOKEN}" \
        -d '{"name":"Coq au Vin","price_cents":3100,"is_available":false}')
    RB=$(body "${R}")
    assert_status "update item → 200"        "200"         "$(status "${R}")"
    assert_eq     "  name updated"           "Coq au Vin"  "$(field "${RB}" "['name']")"
    assert_eq     "  price updated"          "3100"        "$(field "${RB}" "['price_cents']")"
    assert_eq     "  availability updated"   "False"       "$(field "${RB}" "['is_available']")"

    subsection "DELETE /api/v1/restaurants/:id/menu/:item_id — delete item"
    R=$(curl -s --max-time 10 -w "\n%{http_code}" \
        -X DELETE \
        -H "Authorization: Bearer ${TOKEN}" \
        "${BASE_URL}/api/v1/restaurants/${REST_ID}/menu/${ITEM_ID}")
    assert_status "delete item → 204"        "204" "$(status "${R}")"

    # Confirm the menu list no longer contains the item.
    R=$(_GET "${BASE_URL}/api/v1/restaurants/${REST_ID}/menu" \
        -H "Authorization: Bearer ${TOKEN}")
    assert_status "menu list still returns 200 after delete" "200" "$(status "${R}")"

    # ── Cross-tenant isolation ──────────────────────────────────────────────
    subsection "Cross-tenant isolation"
    # Register a second tenant, try to access first tenant's restaurant.
    local REG2
    REG2=$(_POST "${BASE_URL}/api/v1/auth/register" \
        -d '{"business_name":"Other Corp","email":"other@test.dev","password":"password99"}')
    local TOKEN2; TOKEN2=$(field "$(body "${REG2}")" "['access_token']")

    R=$(_GET "${BASE_URL}/api/v1/restaurants/${REST_ID}" \
        -H "Authorization: Bearer ${TOKEN2}")
    assert_status "different tenant cannot see restaurant → 404" "404" "$(status "${R}")"

    R=$(_POST "${BASE_URL}/api/v1/restaurants/${REST_ID}/menu" \
        -H "Authorization: Bearer ${TOKEN2}" \
        -d '{"name":"Stolen Item"}')
    assert_status "different tenant cannot add menu item → 404" "404" "$(status "${R}")"

    # ── Delete restaurant ─────────────────────────────────────────────────────
    subsection "DELETE /api/v1/restaurants/:id"
    R=$(curl -s --max-time 10 -w "\n%{http_code}" \
        -X DELETE \
        -H "Authorization: Bearer ${TOKEN}" \
        "${BASE_URL}/api/v1/restaurants/${REST_ID}")
    assert_status "delete restaurant → 204"           "204" "$(status "${R}")"

    R=$(_GET "${BASE_URL}/api/v1/restaurants/${REST_ID}" \
        -H "Authorization: Bearer ${TOKEN}")
    assert_status "get deleted restaurant → 404"       "404" "$(status "${R}")"

    R=$(curl -s --max-time 10 -w "\n%{http_code}" \
        -X DELETE \
        -H "Authorization: Bearer ${TOKEN}" \
        "${BASE_URL}/api/v1/restaurants/${REST_ID}")
    assert_status "double delete → 404"                "404" "$(status "${R}")"
}

# ── Entry point ──────────────────────────────────────────────────────────────────
main() {
    echo ""
    echo -e "${BOLD}${BLUE}  ╔══════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}${BLUE}  ║         Forgebike Test Suite                   ║${NC}"
    echo -e "${BOLD}${BLUE}  ╚══════════════════════════════════════════════╝${NC}"
    echo -e "  ${DIM}$(date '+%Y-%m-%d %H:%M:%S')   BASE_URL=${BASE_URL}${NC}"

    start_infrastructure
    start_server
    test_phase_0
    test_phase_1
    test_phase_2
    print_summary

    [ "${FAIL}" -gt 0 ] && exit 1 || exit 0
}

main
