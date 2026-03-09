function parseMajor(version) {
  if (!version) return null;
  const raw = String(version).trim().replace(/^v/, '');
  const major = Number(raw.split('.')[0]);
  return Number.isFinite(major) ? major : null;
}

function checkNodeVersion({ minMajor = 20, version = process.versions.node } = {}) {
  const major = parseMajor(version);
  if (!major) {
    return {
      ok: false,
      message: `Unable to parse Node version "${version}". Fluxboard tests require Node ${minMajor}+.`,
    };
  }

  if (major < minMajor) {
    return {
      ok: false,
      message: [
        `Node ${minMajor}+ required for Fluxboard tests (jsdom).`,
        `Current: ${version}`,
        'Upgrade Node and re-run tests.'
      ].join(' ')
    };
  }

  return { ok: true, message: '' };
}

function requireNodeVersion(options) {
  const result = checkNodeVersion(options);
  if (!result.ok) {
    console.error(result.message);
    process.exitCode = 1;
  }
  return result;
}

if (require.main === module) {
  requireNodeVersion();
}

module.exports = {
  checkNodeVersion,
  requireNodeVersion,
};
