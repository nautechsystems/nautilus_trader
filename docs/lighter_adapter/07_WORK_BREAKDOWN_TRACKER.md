
# 07_WORK_BREAKDOWN_TRACKER.md

## Work Breakdown & Tracker

### Milestone 1: Data Foundation (Week 1)

**Definition of Done**: Can load all instruments and subscribe to market data

- [ ] **PR0: Scaffolding**
  - [ ] Create package structure
  - [ ] Implement config classes
  - [ ] Define enums and constants
  - [ ] Add credential utilities
  - [ ] Unit tests for config

- [ ] **PR1: Instruments**
  - [ ] HTTP client with auth
  - [ ] orderBooks endpoint
  - [ ] Parse to CryptoPerpetual
  - [ ] InstrumentProvider implementation
  - [ ] Integration test: load instruments

### Milestone 2: Market Data (Week 2)

**Definition of Done**: Live order book and trade feeds working

- [ ] **PR2: WebSocket + Data Client**
  - [ ] WebSocket client with reconnect
  - [ ] Order book subscription
  - [ ] Trade subscription
  - [ ] Market stats subscription
  - [ ] Offset sequencing
  - [ ] Snapshot fetch + delta sync
  - [ ] Data client implementation
  - [ ] Integration test: order book sync

### Milestone 3: Execution (Week 3)

**Definition of Done**: Can place and cancel orders

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
| Standard rate limits too restrictive | High | Medium | Recommend Premium accounts; implement strict throttling |
| Missing error code docs | Medium | High | Build error catalog through testing; contact Lighter support |
| Nonce race conditions | High | Medium | Implement mutex on nonce access; persist atomically |
| WS no snapshot | Medium | Certain | Always fetch REST snapshot first |
| Auth token expiry | Medium | Medium | Proactive refresh at 7 hours |
| Testnet != Mainnet behavior | Medium | Low | Test on mainnet with small amounts |

---

### Open Questions to Resolve

| # | Question | Priority | Validation Method |
|---|----------|----------|-------------------|
| 1 | Complete error code enumeration | High | Contact Lighter support; test all error paths |
| 2 | WebSocket ping/pong requirements | High | Monitor connection stability; test without keepalive |
| 3 | Can WS deliver initial snapshot? | Medium | Test subscription without prior REST fetch |
| 4 | Nonce behavior on API key rotation | Medium | Test with new key after old key used |
| 5 | Batch transaction limits | Medium | Test sendTxBatch with &gt;50 txs |
| 6 | Premium account upgrade time | Low | Check if immediate or delayed |
| 7 | Testnet reliability SLA | Medium | Monitor testnet availability |
| 8 | Funding payment timing precision | Medium | Observe funding events timing |
| 9 | Self-trade prevention details | Low | Test crossing own orders |
| 10 | Maximum orders per account | Medium | Check accountLimits endpoint |

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