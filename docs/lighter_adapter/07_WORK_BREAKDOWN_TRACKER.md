
# 07_WORK_BREAKDOWN_TRACKER.md

## Work Breakdown & Tracker

### Milestone 1: Data Foundation (Week 1)

**Definition of Done**: Can load all instruments and subscribe to market data

- [x] **PR0: Scaffolding**
  - [x] Create package structure
  - [x] Implement config classes
  - [x] Define enums and constants
  - [x] Add credential utilities
  - [x] Unit tests for config

- [ ] **PR1: Instruments**
  - [ ] HTTP client with auth
  - [ ] orderBooks endpoint
  - [ ] Parse to CryptoPerpetual
  - [ ] InstrumentProvider implementation
  - [ ] Integration test: load instruments

### Milestone 2: Market Data (Week 2)

**Definition of Done**: Live order book and trade feeds working (public)

- [ ] **PR2: WebSocket + Data Client**
  - [ ] WebSocket client with reconnect
  - [ ] Order book subscription
  - [ ] Trade subscription
  - [ ] Market stats subscription
  - [ ] Offset sequencing
  - [ ] Snapshot fetch + delta sync
  - [ ] Data client implementation
  - [ ] Integration test: order book sync (fixture-backed)

### Milestone 2.5: Validation Spike (Week 2)

**Definition of Done**: Signing/auth/WS schema questions answered with real captures

- [ ] Capture successful `sendTx` request/response (hashing + signing recipe)
- [ ] Confirm whether auth token is required for private REST + WS
- [ ] Record WS payloads to lock channel names and snapshot/delta semantics
- [ ] Store redacted fixtures under `tests/test_data/lighter/{http,ws}/`
- [ ] Write doc update summarizing validated behaviors

### Milestone 3: Execution (Week 3)

**Definition of Done**: Can place and cancel orders (gated on Validation Spike)

- [ ] **PR3: Execution Client**
  - [ ] Nonce manager
  - [ ] Order submission (sendTx)
  - [ ] Order cancellation
  - [ ] User order stream (WS)
  - [ ] Status event mapping
  - [ ] Execution client implementation
  - [ ] Integration test: order lifecycle

### Milestone 4: Account (Week 4)

**Definition of Done**: Position and balance tracking complete

- [ ] **PR4: Account Management**
  - [ ] Balance fetching
  - [ ] Position reports
  - [ ] Position updates (WS)
  - [ ] Fill reports
  - [ ] Reconciliation logic
  - [ ] Integration test: position lifecycle

### Milestone 5: Hardening (Week 5-6)

**Definition of Done**: Production-ready adapter

- [ ] **PR5: Hardening**
  - [ ] Reconnect with backoff
  - [ ] Rate limit handling
  - [ ] Auth token refresh
  - [ ] Comprehensive logging
  - [ ] Metrics integration
  - [ ] 24-hour stability test
  - [ ] Documentation

---

### Risk Register

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Signing algorithm unknown | High | High | Validation spike to capture real `sendTx` requests |
| Auth token requirement unclear | High | Medium | Implement token path; test both token/no-token during validation |
| WS schema/channel mismatch | Medium | Medium | Record payloads and adjust parsers before releasing |
| Fee schedule inconsistency | Medium | Medium | Avoid hardcoding; prefer exchange-reported fees once confirmed |
| Standard rate limits too restrictive | High | Medium | Recommend Premium accounts; implement strict throttling |
| Testnet != Mainnet behavior | Medium | Low | Test on mainnet with minimal size post-validation |

---

### Open Questions to Resolve

| # | Question | Priority | Validation Method |
|---|----------|----------|-------------------|
| 1 | Signing algorithm + payload hashing for `sendTx` | High | Capture successful tx and reproduce signature |
| 2 | Are auth tokens required for private REST/WS? | High | Attempt with/without token on testnet |
| 3 | Exact WS channel names and payload schemas | High | Subscribe and record messages (`order_book/0` vs `order_book:0`) |
| 4 | Do WS channels emit snapshots on subscribe? | High | Observe initial messages and compare to REST snapshot |
| 5 | Fee schedule (Standard vs Premium) | Medium | Confirm via support or live fills; prefer exchange-reported fees |
| 6 | Nonce persistence/recovery rules | Medium | Force mismatch and use `nextNonce` (or equivalent) to reconcile |
| 7 | Batch transaction limits and error behavior | Medium | Test `sendTxBatch` with varying sizes |
| 8 | Funding payment timing/precision | Medium | Observe funding events and compare to docs |
| 9 | WS ping/pong expectations | Low | Monitor keepalive needs; add client-side ping if required |
| 10 | Max orders per account / throttling | Low | Probe limits on testnet and document behavior |

---

### How to Test (PR0)

- Command: `python -m pytest tests/unit_tests/adapters/lighter/test_config.py -q`
- Expected: All tests pass (env resolution for API key/account index, invalid env raises ValueError).

---

### GitHub Issue Templates

**Bug Report**:
```markdown
## Bug Report: Lighter Adapter

**Component**: [data/execution/account/websocket]
**Severity**: [critical/high/medium/low]

### Description
[Clear description of the bug]

### Steps to Reproduce
1. 
2. 
3. 

### Expected Behavior
[What should happen]

### Actual Behavior
[What actually happens]

### Logs
```
[Relevant log output]
```

### Environment
- Nautilus version: 
- Python version: 
- Lighter account type: [Standard/Premium]
- Network: [Testnet/Mainnet]
```

**Feature Request**:
```markdown
## Feature Request: Lighter Adapter

**Feature**: [Brief title]

### User Story
As a [persona], I want [feature] so that [benefit].

### Acceptance Criteria
- [ ] Criterion 1
- [ ] Criterion 2

### Technical Notes
[Implementation considerations]

### Priority Justification
[Why this matters]
```

**Investigation Task**:
```markdown
## Investigation: [Topic]

**Goal**: [What we're trying to learn]

### Questions to Answer
1. 
2. 

### Approach
[How to investigate]

### Success Criteria
[How we know we're done]

### Findings
[To be filled after investigation]
```

---
