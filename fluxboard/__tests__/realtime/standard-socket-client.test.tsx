import { renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useStandardWebSocketSubscription } from '@/hooks/useWebSocket';

const socketsMock = vi.hoisted(() => {
  const unsubscribe = vi.fn();
  return {
    socket: {
      on: vi.fn(),
      off: vi.fn(),
      connected: true,
    },
    standardSocketClient: {
      subscribe: vi.fn(() => unsubscribe),
    },
    unsubscribe,
  };
});

vi.mock('@/sockets', () => ({
  socket: socketsMock.socket,
  standardSocketClient: socketsMock.standardSocketClient,
}));

describe('useStandardWebSocketSubscription', () => {
  const lineage = {
    contract_version: 2,
    surface: 'signal',
    profile: 'tokenmm',
    surface_query_key: 'signal:tokenmm:canonical',
    stream_id: 'signal-main',
    snapshot_revision: 17,
    last_seq: 3,
  };

  beforeEach(() => {
    socketsMock.socket.on.mockReset();
    socketsMock.socket.off.mockReset();
    socketsMock.standardSocketClient.subscribe.mockReset();
    socketsMock.unsubscribe.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('subscribes through the shared standard socket client and cleans up on unmount', async () => {
    let currentResumeFromSeq = 3;
    const onEvent = vi.fn();
    const onSubscribed = vi.fn();

    const { unmount } = renderHook(() =>
      useStandardWebSocketSubscription({
        enabled: true,
        lineage,
        resumeFromSeq: () => currentResumeFromSeq,
        onEvent,
        onSubscribed,
      }),
    );

    await waitFor(() => {
      expect(socketsMock.standardSocketClient.subscribe).toHaveBeenCalledTimes(1);
    });

    const options = socketsMock.standardSocketClient.subscribe.mock.calls[0]?.[0];
    expect(options.lineage).toEqual(lineage);
    expect(options.resumeFromSeq()).toBe(3);

    currentResumeFromSeq = 8;
    expect(options.resumeFromSeq()).toBe(8);

    options.onSubscribed?.({ accepted: true, surface: 'signal' });
    expect(onSubscribed).toHaveBeenCalledWith(expect.objectContaining({ accepted: true, surface: 'signal' }));

    options.onEvent?.({ kind: 'delta_batch', surface: 'signal' });
    expect(onEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: 'delta_batch', surface: 'signal' }));

    unmount();
    expect(socketsMock.unsubscribe).toHaveBeenCalledTimes(1);
  });

  it('surfaces subscription failures without silently downgrading to legacy transport', async () => {
    const onFailure = vi.fn();

    renderHook(() =>
      useStandardWebSocketSubscription({
        enabled: true,
        lineage,
        onEvent: vi.fn(),
        onFailure,
      }),
    );

    await waitFor(() => {
      expect(socketsMock.standardSocketClient.subscribe).toHaveBeenCalledTimes(1);
    });

    const options = socketsMock.standardSocketClient.subscribe.mock.calls[0]?.[0];
    options.onFailure?.({
      type: 'subscribe_rejected',
      reason: 'backend_kill_switch',
      requested: {
        contract_version: 2,
        surface: 'signal',
        profile: 'tokenmm',
        surface_query_key: 'signal:tokenmm:canonical',
        stream_id: 'signal-main',
        snapshot_revision: 17,
        resume_from_seq: 3,
      },
      ack: {
        accepted: false,
        reason: 'backend_kill_switch',
      },
    });

    expect(onFailure).toHaveBeenCalledWith(expect.objectContaining({
      type: 'subscribe_rejected',
      reason: 'backend_kill_switch',
    }));
  });
});
