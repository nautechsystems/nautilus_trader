#!/usr/bin/env bash
set -euo pipefail

# Check OSV scanner JSON results for critical/high severity vulnerabilities.
#
# Reads a JSON results file produced by osv-scanner --format json and exits
# non-zero when any CRITICAL, HIGH, or unclassified finding is present.
# Medium and lower findings are logged but do not block.
#
# Severity is determined by:
#   1. database_specific.severity (GitHub advisory labels: CRITICAL, HIGH, etc.)
#   2. CVSS 3.x base score computed from severity[].score vector strings
#      (>= 9.0 critical, >= 7.0 high, >= 4.0 medium, < 4.0 low)
#
# Usage: osv-severity-gate.sh <results.json>

results="${1:?Usage: osv-severity-gate.sh <results.json>}"

if [ ! -f "$results" ]; then
  echo "::error::OSV results file not found: $results"
  exit 1
fi

# Classify each vulnerability and output one line per finding:
#   SEVERITY | ID | package version
#
# Uses database_specific.severity when available, otherwise computes
# CVSS 3.x base score from the vector string via Python.
classify_output=$(
  python3 - "$results" << 'PYTHON'
import json, math, re, sys

def cvss3_base_score(vector):
    """Compute CVSS 3.x base score from a vector string."""
    m = dict(re.findall(r"(\w+):(\w)", vector))
    if not all(k in m for k in ("C", "I", "A")):
        return None

    cia = {"N": 0, "L": 0.22, "H": 0.56}
    conf, integ, avail = cia[m["C"]], cia[m["I"]], cia[m["A"]]

    av = {"N": 0.85, "A": 0.62, "L": 0.55, "P": 0.20}.get(m.get("AV", "N"), 0.85)
    ac = {"L": 0.77, "H": 0.44}.get(m.get("AC", "L"), 0.77)
    ui = {"N": 0.85, "R": 0.62}.get(m.get("UI", "N"), 0.85)
    scope = m.get("S", "U")

    if scope == "C":
        pr = {"N": 0.85, "L": 0.68, "H": 0.50}.get(m.get("PR", "N"), 0.85)
    else:
        pr = {"N": 0.85, "L": 0.62, "H": 0.27}.get(m.get("PR", "N"), 0.85)

    iss = 1 - (1 - conf) * (1 - integ) * (1 - avail)
    if scope == "U":
        impact = 6.42 * iss
    else:
        impact = 7.52 * (iss - 0.029) - 3.25 * (iss - 0.02) ** 15

    if impact <= 0:
        return 0.0

    exploit = 8.22 * av * ac * pr * ui
    if scope == "U":
        raw = min(impact + exploit, 10)
    else:
        raw = min(1.08 * (impact + exploit), 10)

    return math.ceil(raw * 10) / 10

def severity_from_score(score):
    if score >= 9.0:
        return "CRITICAL"
    if score >= 7.0:
        return "HIGH"
    if score >= 4.0:
        return "MEDIUM"
    return "LOW"

data = json.load(open(sys.argv[1]))
for result in data.get("results", []):
    for pkg_info in result.get("packages", []):
        pkg = pkg_info.get("package", {})
        name = pkg.get("name", "?")
        ver = pkg.get("version", "?")
        for vuln in pkg_info.get("vulnerabilities", []):
            vid = vuln.get("id", "?")

            # Try database_specific.severity first
            db_sev = (vuln.get("database_specific") or {}).get("severity", "")
            if db_sev:
                # Normalize GitHub "MODERATE" to "MEDIUM"
                sev = "MEDIUM" if db_sev == "MODERATE" else db_sev.upper()
                print(f"{sev}|{vid}|{name} {ver}")
                continue

            # Fall back to CVSS 3.x vector
            best_score = None
            for s in vuln.get("severity", []):
                if s.get("type", "").startswith("CVSS_V") and "score" in s:
                    score = cvss3_base_score(s["score"])
                    if score is not None:
                        best_score = max(best_score or 0, score)

            if best_score is not None:
                sev = severity_from_score(best_score)
                print(f"{sev}|{vid}|{name} {ver} (CVSS {best_score:.1f})")
            else:
                print(f"UNKNOWN|{vid}|{name} {ver}")
PYTHON
)

critical=0 high=0 medium=0 low=0 unknown=0

if [ -n "$classify_output" ]; then
  while IFS='|' read -r sev vid detail; do
    [ -z "$sev" ] && continue
    case "$sev" in
      CRITICAL) critical=$((critical + 1)) ;;
      HIGH) high=$((high + 1)) ;;
      MEDIUM) medium=$((medium + 1)) ;;
      LOW) low=$((low + 1)) ;;
      *) unknown=$((unknown + 1)) ;;
    esac
    echo "  ${sev} | ${vid} | ${detail}"
  done <<< "$classify_output"
fi

total=$((critical + high + medium + low + unknown))

echo "OSV severity summary: ${critical} critical, ${high} high, ${medium} medium, ${low} low, ${unknown} unknown"

if [ "$total" -eq 0 ]; then
  echo "No unfiltered vulnerabilities found"
  exit 0
fi

if [ "$critical" -gt 0 ] || [ "$high" -gt 0 ] || [ "$unknown" -gt 0 ]; then
  echo "::error::Found ${critical} critical, ${high} high, ${unknown} unknown severity vulnerabilities"
  exit 1
fi

echo "No critical/high findings, not blocking"
