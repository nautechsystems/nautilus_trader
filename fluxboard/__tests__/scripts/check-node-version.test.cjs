// @vitest-environment node

const { checkNodeVersion } = require('../../scripts/check-node-version.cjs');

describe('checkNodeVersion', () => {
  it('fails with a helpful message when below the minimum version', () => {
    const result = checkNodeVersion({ minMajor: 20, version: '18.19.1' });

    expect(result.ok).toBe(false);
    expect(result.message).toMatch(/Node 20\+/);
    expect(result.message).toMatch(/18\.19\.1/);
  });

  it('passes when the version meets the minimum', () => {
    const result = checkNodeVersion({ minMajor: 20, version: '20.4.0' });

    expect(result.ok).toBe(true);
  });
});
