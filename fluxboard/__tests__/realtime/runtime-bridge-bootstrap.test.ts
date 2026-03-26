import { beforeEach, describe, expect, it, vi } from 'vitest';

const registerSharedWebSocketBridge = vi.fn();

vi.mock('@/hooks/useWebSocket', () => ({
  registerSharedWebSocketBridge,
}));

describe('realtime runtime bridge bootstrap', () => {
  beforeEach(() => {
    registerSharedWebSocketBridge.mockReset();
    vi.resetModules();
  });

  it('registers the shared websocket bridge during runtime bootstrap', async () => {
    await import('@/lib/realtime/runtimeBridge');

    expect(registerSharedWebSocketBridge).toHaveBeenCalledTimes(1);
    expect(registerSharedWebSocketBridge).toHaveBeenCalledWith(
      expect.objectContaining({
        subscribe: expect.any(Function),
      }),
    );
  });
});
