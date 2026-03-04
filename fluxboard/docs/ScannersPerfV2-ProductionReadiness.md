<!-- DOCID: ScannersPerfV2-ProductionReadiness.md@v1 -->
Last updated: 2025-12-10 · commit 70da26dc

<!-- DOCID: fluxboard/scanners-perf-v2-readiness@v1 -->

# Scanners Perf V2 - Production Readiness Assessment

## Purpose

Capture the operational checklist for taking Scanners Perf V2 from implemented feature to safely enabled, monitored production behavior.

## Scope

- Pre-production performance and integration validation
- Staging and canary rollout steps
- Monitoring, alerting, and rollback expectations

## Interface

- Feature flag: `VITE_SCANNERS_PERF_V2` / `fluxboard:feature:scanners-perf-v2`
- Metrics source: `fluxboard_perf_exporter.py` (Prometheus exporter)
- Dashboard: `fluxboard_scanners_perf.json` Grafana dashboard
- Backend endpoint: `POST /api/v1/scanners/perf-stats`

## Prereqs

- Scanners Perf V2 implementation merged and behind feature flag
- Exporter and Grafana environment available (or planned) for production
- Access to staging and production deployments

## Procedure

1. Run local/staging performance harness scenarios as described in `ScannersPerfV2.md`.
2. Validate KPIs using the **Pre-Production Checklist** below.
3. Deploy exporter and hook it into Prometheus and Grafana.
4. Enable in staging, soak for 24–48 hours, then run canary rollout in production.

## Validation

- Use the **Performance Validation**, **Integration Testing**, and **Staging Validation** subsections below as the formal gate before enabling the flag by default.
- Confirm alerting is in place for buffer size, dropped deltas, exporter health, and latency.

## Rollback

- Disable the feature flag via env/localStorage override.
- Redeploy frontend if necessary to pick up default-off configuration.
- Follow the **Rollback test** and **Rollout Plan** guidance below to ensure reversibility.

## Troubleshooting

- Use the checklists under **Error Handling & Resilience**, **Alerting & Monitoring**, and **Risk Assessment** below.
- For performance issues, coordinate with the Scanners team using the same KPIs as in `ScannersPerfV2.md`.

## FAQ

- **Q:** Can we ship Perf V2 without exporter/alerts?
  **A:** Not recommended. At minimum, exporter and basic Grafana panels should be in place.

## Examples

- Example rollout sequence: local → staging flag on → canary in production → global enablement.

## References

- Design and architecture: `fluxboard/docs/ScannersPerfV2.md`
- Exporter implementation: `scripts/exporters/fluxboard_perf_exporter.py`
- Architecture doc: `docs/architecture/scanners-performance-improvements.md`

## Changelog

- 2025-11-20: Added standard doc sections and aligned terminology with `ScannersPerfV2.md`.

## ✅ Completed

### Core Implementation
- [x] rAF delta coalescing implemented
- [x] Incremental index updates (O(log n))
- [x] Preformatted display strings
- [x] Optimized age ticker (visibility/idle throttling)
- [x] Performance instrumentation (marks/measures)
- [x] Redis stats publishing
- [x] Feature flag system (`scanners.perfV2`, defaults to `false`)

### Testing
- [x] Backend API unit tests (3 tests)
- [x] Feature flag tests
- [x] Store Perf V2 tests
- [x] Performance harness (dev tool)

### Documentation
- [x] Architecture documentation
- [x] Deployment guide
- [x] Troubleshooting guide
- [x] Testing documentation

### Monitoring Infrastructure
- [x] Grafana dashboard (`fluxboard_scanners_perf.json`)
- [x] Prometheus exporter (`fluxboard_perf_exporter.py`)
- [x] Prometheus scrape config (port 9092)
- [x] Backend API endpoint (`/api/v1/scanners/perf-stats`)

## ⚠️ Pre-Production Checklist

### 1. Performance Validation
- [ ] Run performance harness with 10k @ 100Hz scenario
- [ ] Verify acceptance criteria met:
  - [ ] Avg FPS ≥ 55
  - [ ] Min FPS ≥ 50
  - [ ] Apply p95 < 60ms
  - [ ] Render p95 < 12ms
  - [ ] CPU < 30% main-thread
  - [ ] No >50ms GC pauses
- [ ] Test with real WebSocket data (not just harness)
- [ ] Validate on multiple browsers (Chrome, Firefox, Safari)

### 2. Integration Testing
- [ ] End-to-end test: WebSocket delta → table update → metrics published
- [ ] Test with real scanner data (not synthetic)
- [ ] Verify metrics appear in Grafana
- [ ] Test feature flag toggle (enable/disable mid-session)

### 3. Production Infrastructure
- [ ] Exporter deployed as service/systemd unit
- [ ] Exporter auto-restarts on failure
- [ ] Exporter logs to centralized logging
- [ ] Prometheus scraping exporter successfully
- [ ] Grafana dashboard imported and visible
- [ ] Redis connection verified (production Redis URL)

### 4. Error Handling & Resilience
- [ ] Test behavior when Redis is down (stats publishing fails silently)
- [ ] Test behavior when exporter is down (metrics stop updating)
- [ ] Test behavior when API endpoint returns 429 (rate limiting)
- [ ] Verify graceful degradation (perfV2 disabled falls back to legacy)

### 5. Staging Validation
- [ ] Deploy to staging environment
- [ ] Enable flag: `VITE_SCANNERS_PERF_V2=1`
- [ ] Monitor for 24-48 hours
- [ ] Verify no regressions in:
  - [ ] Table rendering
  - [ ] Filtering/sorting
  - [ ] WebSocket updates
  - [ ] Memory usage
- [ ] Compare metrics vs legacy path

### 6. Alerting & Monitoring
- [ ] Configure alerts for:
  - [ ] High buffer size (>5k sustained)
  - [ ] Slow apply times (p95 > 100ms)
  - [ ] High dropped delta rate (>10%)
  - [ ] Exporter down
- [ ] Set up dashboard alerts in Grafana
- [ ] Document alert response procedures

### 7. Rollout Plan
- [ ] Canary deployment plan (localStorage override for test users)
- [ ] Gradual rollout schedule (10% → 50% → 100%)
- [ ] Rollback procedure tested
- [ ] Communication plan (if needed)

### 8. Documentation Updates
- [ ] Production deployment runbook
- [ ] Alert runbook
- [ ] Known issues/limitations
- [ ] Performance tuning guide

## 🚨 Critical Path Items

**Must complete before production:**
1. **Performance validation** - Run harness, verify acceptance criteria
2. **Staging validation** - 24-48 hour soak test
3. **Exporter deployment** - Ensure it's running and monitored
4. **Rollback test** - Verify feature flag disable works instantly

**Nice to have:**
- Integration tests (can be added post-launch)
- Alerting (can be added post-launch, but recommended)
- Multi-browser testing (can be done post-launch)

## Estimated Time to Production

**If starting from scratch:**
- Performance validation: 2-4 hours
- Staging deployment: 1 hour
- Staging validation: 24-48 hours (soak test)
- Exporter deployment: 1 hour
- **Total: ~3-5 days** (mostly waiting for soak test)

**If infrastructure already set up:**
- Performance validation: 2-4 hours
- Staging validation: 24-48 hours
- **Total: ~2-3 days**

## Risk Assessment

**Low Risk:**
- Feature flag defaults to `false` (opt-in)
- All code gated behind flag check
- Instant rollback via flag disable
- Graceful degradation to legacy path

**Medium Risk:**
- Performance not validated against acceptance criteria yet
- Exporter not deployed/configured in production
- No alerting configured

**Mitigation:**
- Canary rollout (start with 10% of users)
- Monitor Grafana dashboard closely
- Keep legacy path available for 1 release cycle

## Recommendation

**Status: ~80% Production Ready**

**Ready for:**
- Staging deployment
- Performance validation
- Canary rollout (with close monitoring)

**Not ready for:**
- Full production rollout (need staging validation first)
- Removing legacy path (keep for 1 release cycle)

**Next Steps:**
1. Run performance harness validation (2-4 hours)
2. Deploy to staging (1 hour)
3. Enable flag in staging, monitor for 24-48 hours
4. Deploy exporter to production (1 hour)
5. Canary rollout (10% of users via localStorage)
6. Monitor for 48 hours
7. Gradual rollout to 100%
