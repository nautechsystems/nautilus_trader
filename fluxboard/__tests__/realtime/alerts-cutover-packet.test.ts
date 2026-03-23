import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const CUTOVER_PACKET_PATH = resolve(process.cwd(), '../docs/plans/realtime-surfaces/alerts-cutover.md');

describe('alerts cutover packet', () => {
  it('instantiates the required per-surface adoption template fields', () => {
    const packet = readFileSync(CUTOVER_PACKET_PATH, 'utf8');

    expect(packet).toContain('surface_query_key');
    expect(packet).toContain('stream_id');
    expect(packet).toContain('entity ID and delete semantics');
    expect(packet).toContain('authoritative ordering source');
    expect(packet).toContain('snapshot endpoint');
    expect(packet).toContain('live event families used');
    expect(packet).toContain('recovery mode capability');
    expect(packet).toContain('row cap and overscan policy');
    expect(packet).toContain('allowed live sorts and filter rules');
    expect(packet).toContain('degradation triggers and recovery thresholds');
    expect(packet).toContain('health-state UX and action rules');
    expect(packet).toContain('minimum canary cohort');
    expect(packet).toContain('minimum standard-traffic thresholds');
    expect(packet).toContain('required metrics, alert thresholds, dashboards, and rollback playbook refs');
    expect(packet).toContain('current alert state and rollback exercise result');
    expect(packet).toContain('surface cutover readiness checkpoint results');
  });
});
