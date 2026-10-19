#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
EXPECTED_VERSION="$(bash "${REPO_ROOT}/scripts/cargo-tool-version.sh" cbindgen)"

if ! command -v cbindgen > /dev/null 2>&1; then
  echo "cbindgen ${EXPECTED_VERSION} is required but not installed" >&2
  echo "Install with: cargo install cbindgen --version ${EXPECTED_VERSION} --locked" >&2
  exit 1
fi

INSTALLED_VERSION="$(cbindgen --version | awk '{print $2}')"
if [[ "${INSTALLED_VERSION}" != "${EXPECTED_VERSION}" ]]; then
  echo "cbindgen version mismatch: installed ${INSTALLED_VERSION}, expected ${EXPECTED_VERSION}" >&2
  exit 1
fi

CASE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/nautilus-cbindgen-abi.XXXXXX")"
trap 'rm -rf "${CASE_ROOT}"' EXIT

CORE_HEADER="${CASE_ROOT}/core.h"
MODEL_HEADER="${CASE_ROOT}/model.h"

cbindgen \
  --quiet \
  --config "${REPO_ROOT}/crates/core/cbindgen.toml" \
  --output "${CORE_HEADER}" \
  "${REPO_ROOT}/crates/core"
cbindgen \
  --quiet \
  --config "${REPO_ROOT}/crates/model/cbindgen.toml" \
  --output "${MODEL_HEADER}" \
  "${REPO_ROOT}/crates/model"

require_text() {
  local file="$1"
  local expected="$2"

  if ! grep -Fq -- "${expected}" "${file}"; then
    echo "Expected generated header ${file} to contain: ${expected}" >&2
    exit 1
  fi
}

reject_pattern() {
  local file="$1"
  local pattern="$2"

  if grep -Eq -- "${pattern}" "${file}"; then
    echo "Generated header ${file} exposes an internal Rust name matching: ${pattern}" >&2
    exit 1
  fi
}

require_text "${CORE_HEADER}" "typedef struct CVec {"
require_text "${CORE_HEADER}" "typedef struct UUID4_t {"
require_text "${CORE_HEADER}" "typedef struct StackStr {"
require_text "${CORE_HEADER}" "struct UUID4_t uuid4_new(void);"

require_text "${MODEL_HEADER}" "typedef enum OrderSide {"
require_text "${MODEL_HEADER}" "ORDER_SIDE_NO_ORDER_SIDE = 0,"
require_text "${MODEL_HEADER}" "ORDER_SIDE_BUY = 1,"
require_text "${MODEL_HEADER}" "ORDER_SIDE_SELL = 2,"
require_text "${MODEL_HEADER}" "} OrderSide;"
require_text "${MODEL_HEADER}" "typedef enum PositionSide {"
require_text "${MODEL_HEADER}" "POSITION_SIDE_NO_POSITION_SIDE = 0,"
require_text "${MODEL_HEADER}" "POSITION_SIDE_FLAT = 1,"
require_text "${MODEL_HEADER}" "POSITION_SIDE_LONG = 2,"
require_text "${MODEL_HEADER}" "POSITION_SIDE_SHORT = 3,"
require_text "${MODEL_HEADER}" "} PositionSide;"
require_text "${MODEL_HEADER}" "typedef enum ContingencyType {"
require_text "${MODEL_HEADER}" "CONTINGENCY_TYPE_NO_CONTINGENCY = 0,"
require_text "${MODEL_HEADER}" "CONTINGENCY_TYPE_OCO = 1,"
require_text "${MODEL_HEADER}" "CONTINGENCY_TYPE_OTO = 2,"
require_text "${MODEL_HEADER}" "CONTINGENCY_TYPE_OUO = 3,"
require_text "${MODEL_HEADER}" "} ContingencyType;"
require_text "${MODEL_HEADER}" "typedef enum TrailingOffsetType {"
require_text "${MODEL_HEADER}" "TRAILING_OFFSET_TYPE_NO_TRAILING_OFFSET = 0,"
require_text "${MODEL_HEADER}" "TRAILING_OFFSET_TYPE_PRICE = 1,"
require_text "${MODEL_HEADER}" "TRAILING_OFFSET_TYPE_BASIS_POINTS = 2,"
require_text "${MODEL_HEADER}" "TRAILING_OFFSET_TYPE_TICKS = 3,"
require_text "${MODEL_HEADER}" "TRAILING_OFFSET_TYPE_PRICE_TIER = 4,"
require_text "${MODEL_HEADER}" "} TrailingOffsetType;"
require_text "${MODEL_HEADER}" "typedef enum TriggerType {"
require_text "${MODEL_HEADER}" "TRIGGER_TYPE_NO_TRIGGER = 0,"
require_text "${MODEL_HEADER}" "TRIGGER_TYPE_DEFAULT = 1,"
require_text "${MODEL_HEADER}" "TRIGGER_TYPE_LAST_PRICE = 2,"
require_text "${MODEL_HEADER}" "TRIGGER_TYPE_MARK_PRICE = 3,"
require_text "${MODEL_HEADER}" "TRIGGER_TYPE_INDEX_PRICE = 4,"
require_text "${MODEL_HEADER}" "TRIGGER_TYPE_BID_ASK = 5,"
require_text "${MODEL_HEADER}" "TRIGGER_TYPE_DOUBLE_LAST = 6,"
require_text "${MODEL_HEADER}" "TRIGGER_TYPE_DOUBLE_BID_ASK = 7,"
require_text "${MODEL_HEADER}" "TRIGGER_TYPE_LAST_OR_BID_ASK = 8,"
require_text "${MODEL_HEADER}" "TRIGGER_TYPE_MID_POINT = 9,"
require_text "${MODEL_HEADER}" "} TriggerType;"
require_text "${MODEL_HEADER}" "typedef struct BookOrder_t {"
require_text "${MODEL_HEADER}" "typedef struct OrderBookDelta_t {"
require_text "${MODEL_HEADER}" "typedef struct OrderBookDepth10_t {"
require_text "${MODEL_HEADER}" "struct BookOrder_t book_order_new(enum OrderSide order_side,"
require_text "${MODEL_HEADER}" "#define NULL_ORDER"

reject_pattern "${MODEL_HEADER}" "typedef (enum|struct) OrderSideOptional"
reject_pattern "${MODEL_HEADER}" "typedef (enum|struct) PositionSideOptional"
reject_pattern "${MODEL_HEADER}" "typedef (enum|struct) ContingencyTypeOptional"
reject_pattern "${MODEL_HEADER}" "typedef (enum|struct) TrailingOffsetTypeOptional"
reject_pattern "${MODEL_HEADER}" "typedef (enum|struct) TriggerTypeOptional"
reject_pattern "${MODEL_HEADER}" "typedef struct BookOrder BookOrder;"
reject_pattern "${MODEL_HEADER}" "typedef (enum|struct) BookOrderFfi"
reject_pattern "${MODEL_HEADER}" "typedef (enum|struct) OrderBookDeltaFfi"
reject_pattern "${MODEL_HEADER}" "typedef (enum|struct) OrderBookDepth10Ffi"

read -r -a C_COMPILER <<< "${CC:-cc}"
if ! command -v "${C_COMPILER[0]}" > /dev/null 2>&1; then
  echo "C compiler ${C_COMPILER[0]} is required for the generated header smoke test" >&2
  exit 1
fi

cat > "${CASE_ROOT}/abi.c" << 'C'
#include "core.h"
#include "model.h"

_Static_assert(ORDER_SIDE_NO_ORDER_SIDE == 0, "NO_ORDER_SIDE value changed");
_Static_assert(ORDER_SIDE_BUY == 1, "ORDER_SIDE_BUY value changed");
_Static_assert(ORDER_SIDE_SELL == 2, "ORDER_SIDE_SELL value changed");
_Static_assert(POSITION_SIDE_NO_POSITION_SIDE == 0, "NO_POSITION_SIDE value changed");
_Static_assert(POSITION_SIDE_FLAT == 1, "POSITION_SIDE_FLAT value changed");
_Static_assert(POSITION_SIDE_LONG == 2, "POSITION_SIDE_LONG value changed");
_Static_assert(POSITION_SIDE_SHORT == 3, "POSITION_SIDE_SHORT value changed");
_Static_assert(CONTINGENCY_TYPE_NO_CONTINGENCY == 0, "NO_CONTINGENCY value changed");
_Static_assert(CONTINGENCY_TYPE_OCO == 1, "CONTINGENCY_TYPE_OCO value changed");
_Static_assert(CONTINGENCY_TYPE_OTO == 2, "CONTINGENCY_TYPE_OTO value changed");
_Static_assert(CONTINGENCY_TYPE_OUO == 3, "CONTINGENCY_TYPE_OUO value changed");
_Static_assert(TRAILING_OFFSET_TYPE_NO_TRAILING_OFFSET == 0, "NO_TRAILING_OFFSET value changed");
_Static_assert(TRAILING_OFFSET_TYPE_PRICE == 1, "TRAILING_OFFSET_TYPE_PRICE value changed");
_Static_assert(TRAILING_OFFSET_TYPE_BASIS_POINTS == 2, "TRAILING_OFFSET_TYPE_BASIS_POINTS value changed");
_Static_assert(TRAILING_OFFSET_TYPE_TICKS == 3, "TRAILING_OFFSET_TYPE_TICKS value changed");
_Static_assert(TRAILING_OFFSET_TYPE_PRICE_TIER == 4, "TRAILING_OFFSET_TYPE_PRICE_TIER value changed");
_Static_assert(TRIGGER_TYPE_NO_TRIGGER == 0, "NO_TRIGGER value changed");
_Static_assert(TRIGGER_TYPE_DEFAULT == 1, "TRIGGER_TYPE_DEFAULT value changed");
_Static_assert(TRIGGER_TYPE_LAST_PRICE == 2, "TRIGGER_TYPE_LAST_PRICE value changed");
_Static_assert(TRIGGER_TYPE_MARK_PRICE == 3, "TRIGGER_TYPE_MARK_PRICE value changed");
_Static_assert(TRIGGER_TYPE_INDEX_PRICE == 4, "TRIGGER_TYPE_INDEX_PRICE value changed");
_Static_assert(TRIGGER_TYPE_BID_ASK == 5, "TRIGGER_TYPE_BID_ASK value changed");
_Static_assert(TRIGGER_TYPE_DOUBLE_LAST == 6, "TRIGGER_TYPE_DOUBLE_LAST value changed");
_Static_assert(TRIGGER_TYPE_DOUBLE_BID_ASK == 7, "TRIGGER_TYPE_DOUBLE_BID_ASK value changed");
_Static_assert(TRIGGER_TYPE_LAST_OR_BID_ASK == 8, "TRIGGER_TYPE_LAST_OR_BID_ASK value changed");
_Static_assert(TRIGGER_TYPE_MID_POINT == 9, "TRIGGER_TYPE_MID_POINT value changed");

static BookOrder_t (*book_order_new_fn)(OrderSide, Price_t, Quantity_t, uint64_t) = book_order_new;
static BookOrder_t null_order = NULL_ORDER;

int main(void) {
    CVec values = cvec_new();
    BookOrder_t order = {0};
    OrderBookDelta_t delta = {0};
    OrderBookDepth10_t depth = {0};

    (void)values;
    (void)order;
    (void)delta;
    (void)depth;
    (void)book_order_new_fn;
    (void)null_order;
    return 0;
}
C

"${C_COMPILER[@]}" \
  -std=c11 \
  -Werror \
  -fsyntax-only \
  -I "${CASE_ROOT}" \
  "${CASE_ROOT}/abi.c"
"${C_COMPILER[@]}" \
  -std=c11 \
  -Werror \
  -fsyntax-only \
  -DHIGH_PRECISION \
  -I "${CASE_ROOT}" \
  "${CASE_ROOT}/abi.c"

echo "cbindgen C ABI check passed"
