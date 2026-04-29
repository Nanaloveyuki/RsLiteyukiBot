/**
 * Liteyuki project version format.
 * Format: v?A.B.C[-cNNNN][-nickname]
 *
 * Comparison only uses:
 * 1. A (BigVer)
 * 2. B (PublishVer)
 * 3. C (BugFixVer)
 * 4. NNNN (commitVer, starts from 0001)
 *
 * Any nickname suffix is display-only and does not participate in comparison.
 */
const PROJECT_VERSION_REGEX = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-c(\d+))?(?:-(.+))?$/;

interface ProjectVersionInfo {
  valid: boolean;
  normalized: string;
  major: number;
  minor: number;
  patch: number;
  commit: number;
  nickname: string | null;
}

const INVALID_VERSION: ProjectVersionInfo = {
  valid: false,
  normalized: '0.0.0-c0000',
  major: 0,
  minor: 0,
  patch: 0,
  commit: 0,
  nickname: null,
};

/**
 * Parse the project version string.
 * Missing commit suffix is treated as c0000 for backward compatibility with
 * older plain `A.B.C` versions.
 */
export const parseVersion = (version: string | undefined | null): ProjectVersionInfo => {
  if (!version || typeof version !== 'string') {
    return INVALID_VERSION;
  }

  const match = version.trim().match(PROJECT_VERSION_REGEX);
  if (!match) {
    return INVALID_VERSION;
  }

  const major = parseInt(match[1]!, 10);
  const minor = parseInt(match[2]!, 10);
  const patch = parseInt(match[3]!, 10);
  const commit = match[4] ? parseInt(match[4], 10) : 0;
  const nickname = match[5] || null;

  let normalized = `${major}.${minor}.${patch}`;
  if (commit > 0) {
    normalized += `-c${String(commit).padStart(4, '0')}`;
  }
  if (nickname) {
    normalized += `-${nickname}`;
  }

  return {
    valid: true,
    normalized,
    major,
    minor,
    patch,
    commit,
    nickname,
  };
};

/**
 * Version to numeric value for legacy callers.
 */
export const versionToNumber = (version: string): number => {
  const info = parseVersion(version);
  return info.commit + info.patch * 10000 + info.minor * 100000000 + info.major * 1000000000000;
};

/**
 * Compare project versions using A.B.C-cNNNN ordering only.
 * Nickname suffix is ignored.
 */
export const compareVersion = (version1: string, version2: string): -1 | 0 | 1 => {
  const a = parseVersion(version1);
  const b = parseVersion(version2);

  if (!a.valid || !b.valid) {
    return 0;
  }

  if (a.major !== b.major) return a.major > b.major ? 1 : -1;
  if (a.minor !== b.minor) return a.minor > b.minor ? 1 : -1;
  if (a.patch !== b.patch) return a.patch > b.patch ? 1 : -1;
  if (a.commit !== b.commit) return a.commit > b.commit ? 1 : -1;

  return 0;
};

/**
 * Whether the target version is newer than the current version.
 */
export const hasNewVersion = (currentVersion: string, latestVersion: string): boolean => {
  const current = parseVersion(currentVersion);
  const latest = parseVersion(latestVersion);

  if (!current.valid || !latest.valid) {
    return false;
  }

  return compareVersion(latestVersion, currentVersion) > 0;
};
