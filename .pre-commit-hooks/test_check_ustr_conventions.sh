#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "ERROR: ripgrep is required for Ustr convention hook tests" >&2
  echo "       install from: https://github.com/BurntSushi/ripgrep#installation" >&2
  exit 1
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/.pre-commit-hooks/check_ustr_conventions.sh"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

write_rs() {
  local path="$1"
  shift

  mkdir -p "$(dirname "$path")"
  printf '%s\n' "$@" > "$path"
}

run_hook() {
  local case_dir="$1"
  local output="$case_dir/output.txt"

  (cd "$case_dir" && bash "$HOOK") > "$output" 2>&1
}

expect_failure() {
  local case_dir="$1"
  shift

  if run_hook "$case_dir"; then
    echo "Expected Ustr convention hook to fail in $case_dir"
    cat "$case_dir/output.txt"
    exit 1
  fi

  local pattern
  for pattern in "$@"; do
    if ! rg -q "$pattern" "$case_dir/output.txt"; then
      echo "Expected pattern '$pattern' in $case_dir output"
      cat "$case_dir/output.txt"
      exit 1
    fi
  done
}

expect_success() {
  local case_dir="$1"

  if ! run_hook "$case_dir"; then
    echo "Expected Ustr convention hook to pass in $case_dir"
    cat "$case_dir/output.txt"
    exit 1
  fi
}

reject_inner_as_str_case="$TMP_DIR/reject-inner-as-str"
write_rs "$reject_inner_as_str_case/crates/system/src/trader.rs" \
  'pub fn register(strategy_id: StrategyId) {' \
  '    let actor_id = Ustr::from(strategy_id.inner().as_str());' \
  '}'
expect_failure "$reject_inner_as_str_case" "rule1"

reject_ref_inner_case="$TMP_DIR/reject-ref-inner"
write_rs "$reject_ref_inner_case/crates/system/src/trader.rs" \
  'pub fn register(strategy_id: StrategyId) {' \
  '    let actor_id = Ustr::from(&strategy_id.inner());' \
  '}'
expect_failure "$reject_ref_inner_case" "rule1"

reject_qualified_inner_case="$TMP_DIR/reject-qualified-inner"
write_rs "$reject_qualified_inner_case/crates/system/src/trader.rs" \
  'pub fn register(strategy_id: StrategyId) {' \
  '    let actor_id = ustr::Ustr::from(strategy_id.inner().as_str());' \
  '}'
expect_failure "$reject_qualified_inner_case" "rule1"

reject_field_ref_case="$TMP_DIR/reject-field-ref"
write_rs "$reject_field_ref_case/crates/adapters/okx/src/data.rs" \
  'pub fn handle(okx_inst: &OKXInstrument) {' \
  '    let inst_key = Ustr::from(&okx_inst.inst_id);' \
  '}'
expect_failure "$reject_field_ref_case" "rule2"

reject_field_as_str_case="$TMP_DIR/reject-field-as-str"
write_rs "$reject_field_as_str_case/crates/adapters/binance/src/futures/websocket/dispatch.rs" \
  'pub fn dispatch(msg: &BinanceFuturesTradeLiteMsg) {' \
  '    let symbol_ustr = ustr::Ustr::from(msg.symbol.as_str());' \
  '}'
expect_failure "$reject_field_as_str_case" "rule2"

reject_nested_field_case="$TMP_DIR/reject-nested-field"
write_rs "$reject_nested_field_case/crates/adapters/binance/src/futures/websocket/dispatch.rs" \
  'pub fn dispatch(msg: &BinanceFuturesOrderUpdateMsg) {' \
  '    let symbol_ustr = Ustr::from(&msg.order.symbol);' \
  '}'
expect_failure "$reject_nested_field_case" "rule2"

reject_after_gated_import_case="$TMP_DIR/reject-after-gated-import"
write_rs "$reject_after_gated_import_case/crates/adapters/okx/src/data.rs" \
  '#[cfg(test)]' \
  'use nautilus_model::types::Currency;' \
  '' \
  'pub fn handle(okx_inst: &OKXInstrument) {' \
  '    let inst_key = Ustr::from(&okx_inst.inst_id);' \
  '}'
expect_failure "$reject_after_gated_import_case" "data\.rs:5" "Found 1 Ustr convention violation"

reject_after_gated_fn_case="$TMP_DIR/reject-after-gated-fn"
write_rs "$reject_after_gated_fn_case/crates/adapters/okx/src/data.rs" \
  '#[cfg(test)]' \
  'fn is_order_updated(' \
  '    okx_inst: &OKXInstrument,' \
  ') -> Ustr {' \
  '    Ustr::from(&okx_inst.inst_id)' \
  '}' \
  '' \
  'pub fn handle(okx_inst: &OKXInstrument) {' \
  '    let inst_key = Ustr::from(&okx_inst.inst_id);' \
  '}'
expect_failure "$reject_after_gated_fn_case" "data\.rs:9" "Found 1 Ustr convention violation"

reject_after_gated_module_case="$TMP_DIR/reject-after-gated-module"
write_rs "$reject_after_gated_module_case/crates/adapters/okx/src/data.rs" \
  '#[cfg(test)]' \
  'mod tests {' \
  '    fn t(okx_inst: &OKXInstrument) -> Ustr {' \
  '        Ustr::from(&okx_inst.inst_id)' \
  '    }' \
  '}' \
  '' \
  'pub fn handle(okx_inst: &OKXInstrument) {' \
  '    let inst_key = Ustr::from(&okx_inst.inst_id);' \
  '}'
expect_failure "$reject_after_gated_module_case" "data\.rs:9" "Found 1 Ustr convention violation"

reject_after_gated_not_test_case="$TMP_DIR/reject-after-gated-not-test"
write_rs "$reject_after_gated_not_test_case/crates/adapters/okx/src/data.rs" \
  '#[cfg(not(test))]' \
  'pub fn handle(okx_inst: &OKXInstrument) {' \
  '    let inst_key = Ustr::from(&okx_inst.inst_id);' \
  '}'
expect_failure "$reject_after_gated_not_test_case" "data\.rs:3" "Found 1 Ustr convention violation"

reject_after_feature_gate_case="$TMP_DIR/reject-after-feature-gate"
write_rs "$reject_after_feature_gate_case/crates/adapters/okx/src/data.rs" \
  '#[cfg(feature = "testers")]' \
  'pub fn handle(okx_inst: &OKXInstrument) {' \
  '    let inst_key = Ustr::from(&okx_inst.inst_id);' \
  '}'
expect_failure "$reject_after_feature_gate_case" "data\.rs:3" "Found 1 Ustr convention violation"

reject_after_any_test_gate_case="$TMP_DIR/reject-after-any-test-gate"
write_rs "$reject_after_any_test_gate_case/crates/adapters/okx/src/data.rs" \
  '#[cfg(any(feature = "live", test))]' \
  'pub fn handle(okx_inst: &OKXInstrument) {' \
  '    let inst_key = Ustr::from(&okx_inst.inst_id);' \
  '}'
expect_failure "$reject_after_any_test_gate_case" "data\.rs:3" "Found 1 Ustr convention violation"

reject_after_comment_mention_case="$TMP_DIR/reject-after-comment-mention"
write_rs "$reject_after_comment_mention_case/crates/adapters/okx/src/data.rs" \
  '//! The module is gated by #[cfg(test)] and is not part of the public API.' \
  '' \
  'pub fn handle(okx_inst: &OKXInstrument) {' \
  '    let inst_key = Ustr::from(&okx_inst.inst_id);' \
  '}'
expect_failure "$reject_after_comment_mention_case" "data\.rs:4" "Found 1 Ustr convention violation"

allow_marker_case="$TMP_DIR/allow-marker"
write_rs "$allow_marker_case/crates/adapters/deribit/src/http/parse.rs" \
  'pub fn parse(book: &DeribitOrderBook) -> Ustr {' \
  '    // String-typed instrument_name on this struct' \
  '    Ustr::from(&book.instrument_name) // ustr-ok' \
  '}'
expect_success "$allow_marker_case"

allow_non_curated_field_case="$TMP_DIR/allow-non-curated-field"
write_rs "$allow_non_curated_field_case/crates/adapters/binance/src/spot/websocket/trading/client.rs" \
  'pub fn parse(report: &BinanceOrderRejected) -> Ustr {' \
  '    Ustr::from(&report.reject_reason)' \
  '}'
expect_success "$allow_non_curated_field_case"

allow_curated_field_outside_prefix_case="$TMP_DIR/allow-curated-field-outside-prefix"
write_rs "$allow_curated_field_outside_prefix_case/crates/adapters/kraken/src/websocket/spot_v2/client.rs" \
  'pub fn resync(request: &BookResyncRequest) -> Ustr {' \
  '    Ustr::from(&request.symbol)' \
  '}'
expect_success "$allow_curated_field_outside_prefix_case"

allow_first_intern_case="$TMP_DIR/allow-first-intern"
write_rs "$allow_first_intern_case/crates/adapters/okx/src/data.rs" \
  'pub fn handle(s: &str, m: &PerpetualMarket) -> Ustr {' \
  '    let a = Ustr::from(s);' \
  '    let b = Ustr::from(&m.ticker);' \
  '    let c = Ustr::from("LITERAL");' \
  '    a + b + c' \
  '}'
expect_success "$allow_first_intern_case"

allow_test_module_case="$TMP_DIR/allow-test-module"
write_rs "$allow_test_module_case/crates/adapters/okx/src/data.rs" \
  'pub fn prod() {}' \
  '' \
  '#[cfg(test)]' \
  'mod tests {' \
  '    fn t(strategy_id: StrategyId) -> Ustr {' \
  '        Ustr::from(strategy_id.inner().as_str())' \
  '    }' \
  '}'
expect_success "$allow_test_module_case"

allow_multiline_attribute_module_case="$TMP_DIR/allow-multiline-attribute-module"
write_rs "$allow_multiline_attribute_module_case/crates/adapters/okx/src/data.rs" \
  '#[cfg(test)]' \
  '#[expect(' \
  '    clippy::unnecessary_to_owned,' \
  '    reason = "Required for trait bound satisfaction"' \
  ')]' \
  'mod tests {' \
  '    fn t(okx_inst: &OKXInstrument) -> Ustr {' \
  '        Ustr::from(&okx_inst.inst_id)' \
  '    }' \
  '' \
  '    fn u(strategy_id: StrategyId) -> Ustr {' \
  '        Ustr::from(strategy_id.inner().as_str())' \
  '    }' \
  '}'
expect_success "$allow_multiline_attribute_module_case"

allow_cfg_all_test_module_case="$TMP_DIR/allow-cfg-all-test-module"
write_rs "$allow_cfg_all_test_module_case/crates/adapters/okx/src/data.rs" \
  '#[cfg(all(test, feature = "python"))]' \
  'mod tests {' \
  '    fn t(okx_inst: &OKXInstrument) -> Ustr {' \
  '        Ustr::from(&okx_inst.inst_id)' \
  '    }' \
  '}'
expect_success "$allow_cfg_all_test_module_case"

allow_cfg_all_test_last_module_case="$TMP_DIR/allow-cfg-all-test-last-module"
write_rs "$allow_cfg_all_test_last_module_case/crates/adapters/okx/src/data.rs" \
  '#[cfg(all(feature = "python", test))]' \
  'mod tests {' \
  '    fn t(okx_inst: &OKXInstrument) -> Ustr {' \
  '        Ustr::from(&okx_inst.inst_id)' \
  '    }' \
  '}'
expect_success "$allow_cfg_all_test_last_module_case"

allow_doc_comment_between_case="$TMP_DIR/allow-doc-comment-between"
write_rs "$allow_doc_comment_between_case/crates/adapters/okx/src/data.rs" \
  '#[cfg(test)]' \
  '/// Builds a key for tests, as in: let key = Ustr::from(inst.inst_id.as_str());' \
  'fn t(okx_inst: &OKXInstrument) -> Ustr {' \
  '    Ustr::from(&okx_inst.inst_id)' \
  '}'
expect_success "$allow_doc_comment_between_case"

allow_tests_dir_case="$TMP_DIR/allow-tests-dir"
write_rs "$allow_tests_dir_case/crates/adapters/okx/tests/exec_client.rs" \
  'fn t(strategy_id: StrategyId) -> Ustr {' \
  '    Ustr::from(strategy_id.inner().as_str())' \
  '}'
expect_success "$allow_tests_dir_case"

echo "✓ All Ustr convention hook tests passed"
