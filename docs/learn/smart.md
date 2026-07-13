# Participant tracking

## Models

```text
Participant
- id: ParticipantId
- kind: ParticipantKind
- first_seen_at
- last_seen_at

ParticipantProfile
- participant_id
- balances
- positions
- open_orders
- recent_trades
- transactions
- ts_init
```

`ParticipantId` is the canonical identifier. `ParticipantProfile` is a bounded
snapshot of the public data currently available for that participant.

## Ownership

- The adapter discovers participants and owns profile scheduling, venue I/O,
  rate limiting, retries, and normalization.
- DataEngine validates participant data, writes it to Cache, publishes it to
  MessageBus, and routes profile subscription batches to the source adapter.
- The live runner transports commands and `DataEvent` values between them.
- Adapter tasks never read or mutate the shared Cache.

## Data flow

```text
DISCOVERY

Venue activity
  -> source adapter normalizes a Participant batch
  -> DataEvent { source_client_id, participants }
  -> live runner
  -> DataEngine::on_participants
       -> DataEngine::on_participant for each value
       -> validate, upsert in Cache, and publish to MessageBus
       -> collect the accepted ParticipantId values
       -> if non-empty, call subscribe_participant_profiles(participant_ids) once
  -> adapter command handler enqueues the batch and returns immediately

PROFILE ENRICHMENT

Adapter profile worker
  -> deduplicate IDs into the adapter's tracked set
  -> select participants whose profile refresh is due
  -> run bounded asynchronous venue requests
  -> assemble ParticipantProfile
  -> DataEvent::Data(Data::ParticipantProfile(profile))
  -> live runner
  -> DataEngine::on_participant_profile
  -> upsert in Cache and publish to MessageBus
```

The command back to the source adapter is intentional. `on_participants` is the
validation and cache gate; only accepted participants enter profile tracking.
Because discovery already arrives as a batch, DataEngine sends one command for
that batch. It has no participant refresh timer, pending queue, Cache scan, or
in-flight profile state.

`subscribe_participant_profiles` starts continuous adapter-owned refreshes. It
is not a one-shot request, so completed profiles are live `DataEvent` values and
do not require request-response correlation.

## Worker lifecycle

- Each adapter has one profile worker, not one task per participant.
- The worker starts on connect and is cancelled and awaited on disconnect.
- The tracked set belongs to the adapter and survives a transient reconnect;
  reset or disposal clears it.
- The worker deduplicates subscription batches and tracks refresh deadlines and
  in-flight IDs.
- The tracked set and pending work are capped; inactive participants expire or
  an explicit unsubscribe removes them.
- Concurrency, request rate, refresh intervals, and retry backoff are bounded by
  adapter configuration.
- `recent_trades` and `transactions` are bounded windows, never full histories.
- Optional profile fields distinguish unavailable data (`None`) from a
  successful query with no records (`Some([])`).

The first Cache implementation is in memory. PostgreSQL persistence can be
added behind Cache without changing this flow.

