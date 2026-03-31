# AWS-Managed Production Boxes Design

**Date:** 2026-03-26

## Goal

Define a production operating model for the current TokenMM box and future production or pilot boxes that is:

- cheap enough to justify at the current trading scale
- safe for trading latency and correctness
- operationally boring for the box operator
- easy to expand later without redoing the architecture

The immediate problem is not "we need a full managed database platform now." The immediate problem is that the current box has unbounded retention and is acting like an execution node, archive node, and build machine at the same time.

## Constraints And Success Criteria

### Hard constraints

- No synchronous AWS dependency may sit on the trading hot path.
- Strategies, risk, market-data handling, and order state must remain local-first.
- The box must continue trading through temporary AWS or network impairment after startup.
- The current trading scale does not justify a high fixed monthly platform bill.
- The PR must automate provisioning, retention, lifecycle policies, host configuration, and cutover. No manual AWS console steps.
- Future boxes should come up from the same bootstrap path and immediately join the same central operational surfaces.

### Success criteria

- Local disk stops growing without bound.
- The box keeps only the data needed for live trading and short-horizon debugging.
- The important records are durable off-box and queryable centrally.
- Logs are retained only for an operationally useful window, not indefinitely.
- The default monthly cost stays in the "tens of dollars" range rather than immediately committing to an always-on database bill.

## Current State

### Disk evidence from the current host

- Root volume: `629G` total, `366G` used, `263G` free, `59%` used.
- Primary active consumer: `/var/lib/nautilus/telemetry/tokenmm` at roughly `112G`.
- Largest live telemetry files:
  - `quote_cycles.sqlite`: roughly `67.5G`
  - `orders.sqlite`: roughly `15.1G`
  - `balance_snapshots.sqlite`: roughly `28.1G`, stale since `2026-03-18`
- Secondary active consumer: `/var/log` at roughly `6.3G`, dominated by `syslog` rotation rather than capped `journald`.
- Non-runtime headroom loss outside `/var` remains large:
  - `~/releases`: roughly `63G`
  - `~/nautilus_trader/.worktrees`: roughly `92G`

### What that means

- The current pain is mostly a retention problem, not a "we are missing a premium managed service" problem.
- Database storage itself is not the expensive part.
- Managed relational database compute is the expensive part.
- The current host is keeping a lot of data that is either duplicated, stale, or not valuable enough to retain forever.

## What Is Important To Keep

### Must keep long-term

- `orders`
- `fills`
- order acknowledgements, cancel outcomes, and other trading outcome records
- low-frequency `balance_snapshots`
- low-frequency `portfolio_inventory`
- deploy and run metadata needed to correlate behavior to release or host state
- derived summaries such as markouts, quote-cycle rollups, error counters, and daily aggregates

These are the records that explain what the system actually did, what inventory and balance state looked like, and how behavior changed over time.

### Useful, but only for a short window

- raw service logs
- raw `quote_cycle` event history
- verbose request and access logs
- ad hoc debug output

These are useful for incident response and short-horizon strategy analysis, but the value drops quickly. They should be retained for days, not automatically forever.

### Not important to keep on a prod box

- duplicate `syslog` history when `journald` is already present
- old release roots beyond `current + previous`
- git worktrees
- source checkouts as live runtime roots
- stale local SQLite files that are no longer being written
- local archives and build artifacts

## Approaches Considered

### 1. Full managed SQL now

Shape:

- keep local SQLite as a spool
- ship all important telemetry to RDS PostgreSQL
- optionally keep raw quote cycles in S3

Pros:

- straightforward SQL access
- familiar operator model
- close to the repo's current structured telemetry direction

Cons:

- commits the stack to an always-on fixed monthly bill immediately
- wrong economic shape for the current stage
- risks paying for central SQL before there is a real daily operational need for it

### 2. Lean S3 and Athena first, recommended

Shape:

- keep trading-local state local
- export durable history to Parquet in S3
- query centrally through Athena
- retain logs for a short window only
- defer always-on RDS until there is a proven need

Pros:

- low fixed cost
- managed and centralized
- keeps the hot path clean
- gives one cheap durable archive for current and future boxes
- easy to promote later into a richer central data platform

Cons:

- Athena is less ergonomic than a live relational database for some day-to-day workflows
- requires explicit dataset partitioning and retention policy design

### 3. Local cleanup only

Shape:

- prune logs
- prune SQLite
- keep everything else mostly local

Pros:

- cheapest immediate fix
- smallest code change

Cons:

- does not give future boxes a central history
- does not solve off-box durability cleanly
- turns into the same problem again as the system expands

## Chosen Design

### 1. Hard Runtime Boundary

Everything required to decide, risk-check, place, cancel, and reconcile orders remains local to the box.

This includes:

- market-data state
- order book and fair-value state
- risk state
- in-flight order and cancel state
- venue and account reconciliation inputs
- boot-time config cache after startup

AWS is strictly off the hot path. If AWS is unavailable, trading continues and history lags until export resumes.

### 2. Minimal AWS Control Plane

The default AWS-managed control plane for production boxes is:

- `Amazon S3`
- `Amazon Athena`
- `AWS Secrets Manager`
- `AWS Systems Manager`
- `Amazon CloudWatch Logs`, `CloudWatch Metrics`, and `CloudWatch Alarms`
- existing `Amazon ElastiCache` surfaces where the live stack already depends on them

`Amazon RDS for PostgreSQL` is explicitly **deferred** from the default rollout. It becomes a later upgrade only when central SQL is used often enough to justify the fixed monthly cost.

### 3. Data Architecture By Importance

#### Must-keep datasets

Datasets:

- `orders`
- `fills`
- low-frequency `balance_snapshots`
- low-frequency `portfolio_inventory`
- deploy and run metadata
- compact markouts and quote-cycle summary tables

Write path:

- local SQLite spool on the box
- asynchronous exporter
- Parquet in S3
- Athena external tables for central query

Default retention:

- `180-365+ days` depending on the dataset
- can be indefinite for the small core tables if the storage footprint stays small

Why:

- these records are the durable operational history
- they are cheap to store in S3
- they do not require an always-on database instance at the current scale

#### Short-lived raw quote-cycle history

Dataset:

- full-fidelity `quote_cycle` event history

Write path:

- current live writer stays local-first
- quote-cycle SQLite is rotated on a bounded cadence
- rotated segments are converted to Parquet and uploaded to S3
- Athena can query the raw history during the retained window
- compact rollups are retained much longer than the raw events

Default retention:

- local: `24-72 hours`
- S3 raw archive: `7 days` by default
- temporary increase to `30 days` only during active research or incident windows

Why:

- raw quote-cycle history is useful, but not valuable enough to retain forever by default
- short retention preserves debugging and analysis value without turning telemetry into an open-ended cost center

#### Logs

Write path:

- local source of truth: `journald`
- central sink: CloudWatch Logs via Fluent Bit
- no persistent duplicate syslog history

Default retention:

- local journald: `2-3 days` by cap, not by unbounded growth
- CloudWatch Logs: `7 days` default, optionally `14 days` for selected services

Policy:

- do not ship noisy access logs unless they are sampled or intentionally enabled
- keep request-level HTTP logs out of the default long-lived signal
- prioritize strategy, execution, error, and lifecycle logs

Why:

- logs are for operational debugging, not long-term warehousing
- cost is driven by ingestion rate, not just storage
- current `syslog` growth is mostly noise and duplication

### 4. Operator Access Model

### Day-to-day historical access

- Use Athena for:
  - orders
  - fills
  - balance and inventory snapshots
  - run and deploy metadata
  - quote-cycle summary tables

### Deep-dive short-horizon analysis

- Use Athena for:
  - raw quote-cycle history during the retained S3 window

### Logs and host debugging

- Use CloudWatch Logs Insights for central log search over the short retention window.
- Use Pulse and `journald` for on-box live debugging.
- Use SSM Session Manager instead of SSH where practical.

This is not as convenient as a live PostgreSQL database, but it is much cheaper now and still gives central SQL access. When central SQL becomes a daily need, RDS can be added later without changing the hot-path design.

### 5. Local Disk Contract

Production boxes keep only:

- active release root plus previous release root
- bounded `journald` retention
- bounded structured telemetry spool
- bounded quote-cycle spool before archive
- small exporter state and minimal runtime state

Production boxes do not keep:

- git worktrees
- mutable source repos as live roots
- long local log history
- long local SQLite history
- archives beyond explicit bounded retention

### 6. Box Bootstrap And Operator Contract

The PR must make the box operationally boring.

The supported operator workflow is:

1. Merge the PR.
2. Run the standard stack deploy or rollout entrypoint from the repo.
3. Let the scripts create or reuse the S3 bucket and prefixes, CloudWatch log groups, Athena database and tables, secrets and parameters, host baseline, retention policies, and service env files.
4. Let the scripts rotate and prune local state to the configured limits.
5. Verify from Pulse, CloudWatch, and Athena.

Unsupported operator workflow:

- manual AWS console clicking
- manual `/etc/flux/*.env` edits
- manual CloudWatch log-group creation
- manual Athena table creation
- manual lifecycle-policy setup
- manual backup or export commands

### 7. Future Box Standard

Every future prod or pilot box should:

- use the same SSM and Secrets Manager bootstrap pattern
- use the same CloudWatch and Fluent Bit host baseline
- export into the same S3 layout and Athena catalog with profile and host identity fields
- ship from pinned immutable release roots only

That makes a future box "just another executor" rather than a unique pet machine.

### 8. When RDS Becomes Worth It

RDS should be added later, not now, if at least one of these becomes true:

- operators are querying central history every day and Athena friction is real
- multiple boxes need frequent low-latency relational joins
- dashboards and downstream consumers need a continuously queryable SQL database
- the cost of engineer time lost to ad hoc query friction exceeds the fixed monthly cost of RDS

At today's scale, the current system does not appear to justify that step yet.

## Cost Estimates

These are ballpark monthly estimates for `ap-southeast-1` using official AWS public price list data current on `2026-03-26`.

### Pricing inputs used

- S3 Standard storage:
  - `$0.025` per GB-month for the first 50 TB
- Athena:
  - `$5.00` per TB scanned
- CloudWatch Logs custom log ingestion:
  - `$0.70` per GB ingested
- CloudWatch custom metrics:
  - `$0.30` per metric-month for the first 10,000 metrics
- CloudWatch standard alarms:
  - `$0.10` per alarm-metric month
- Secrets Manager:
  - `$0.40` per secret-month
- Optional future RDS PostgreSQL `db.t4g.large`:
  - Single-AZ compute: `$0.203` per hour
  - Single-AZ storage: `$0.138` per GB-month
  - Multi-AZ compute: `$0.406` per hour
  - Multi-AZ storage: `$0.276` per GB-month

### Recommended current-state footprint

Assumptions:

- `250 GB` in S3 for durable history plus recent raw quote-cycle archive
- `1 TB` Athena scanned per month
- `10 GB` CloudWatch Logs ingestion per month after log cleanup
- `10` custom metrics
- `10` standard alarms
- `3` Secrets Manager secrets

Estimated monthly total:

- S3 storage: about `$6`
- Athena scans: about `$5`
- CloudWatch Logs ingestion: about `$7`
- CloudWatch metrics and alarms: about `$4`
- Secrets Manager: about `$1`

Estimated total: about `$23/month`

### Same design with noisier logs

If logs are left at roughly `30 GB/month`:

- CloudWatch Logs ingestion rises to about `$21/month`

Estimated total: about `$37/month`

If a larger working archive footprint grows S3 to `1 TB` while logs stay sane:

- S3 storage rises to about `$25/month`

Estimated total: about `$42/month`

### Optional future RDS step

If RDS PostgreSQL is added later at `db.t4g.large` with `100 GB` GP3 storage:

- Single-AZ compute plus storage: about `$162/month`
- Multi-AZ compute plus storage: about `$324/month`

That would be **in addition to** the lean S3, Athena, CloudWatch, and secrets costs above.

### Design implication

- S3 storage is cheap.
- Athena can provide central SQL access cheaply at the current scale.
- CloudWatch Logs cost is mainly an ingestion control problem.
- The large fixed cost is the always-on managed relational database instance.

That is why the current design should be S3 and Athena first, with RDS deferred.

## Source Links For Costing

- RDS public pricing index: `https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonRDS/current/ap-southeast-1/index.json`
- S3 public pricing index: `https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonS3/current/ap-southeast-1/index.json`
- Athena public pricing index: `https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonAthena/current/ap-southeast-1/index.json`
- CloudWatch public pricing index: `https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonCloudWatch/current/ap-southeast-1/index.json`
- Secrets Manager public pricing index: `https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AWSSecretsManager/current/ap-southeast-1/index.json`

## Acceptance Criteria

- The box trades normally with all AWS services healthy.
- The box also keeps trading if S3, Athena, CloudWatch Logs, or Secrets Manager are temporarily impaired after startup.
- Local disk stays within explicit budgets automatically.
- Operators can query important history centrally without touching local SQLite.
- Logs are retained only for a short operational window.
- A future box can be bootstrapped by the same repo automation without manual AWS configuration.
