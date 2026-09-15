import { describe, expect, it } from 'vitest';

import { RepoStore, type RepoStatus } from './repos.svelte';

/*
 * The four `apply*` methods are where an event from Rust becomes state on this
 * side, and they are the one part of this store that does not need a backend
 * to run: synchronous, plain objects in, `$state` out. `vite.config.ts` excuses
 * the stores as "talking to Tauri", which is true of `open`/`write`/`fetchAll`
 * and not of these.
 *
 * What is pinned here is the set of rules that fail *silently*. A dropped
 * stale-root guard shows another folder's rows; a backwards `isNewRoot` moves
 * the selection on every reload; a half-applied statuses/errors swap leaves a
 * repo both broken and fine at once. None of the three throws, and none is
 * visible in a screenshot of the good case.
 */

function status(overrides: Partial<RepoStatus> = {}): RepoStatus {
  return {
    branch: 'main',
    head: 'abc1234',
    upstream: 'origin/main',
    ahead: 0,
    behind: 0,
    staged: 0,
    unstaged: 0,
    untracked: 0,
    conflicted: 0,
    changedFiles: 0,
    ...overrides,
  };
}

function rootView(overrides: Partial<Parameters<RepoStore['applyRoot']>[0]> = {}) {
  return {
    path: 'C:/dev/code',
    repos: [
      { id: 'api', name: 'api', path: 'C:/dev/code/api' },
      { id: 'web', name: 'web', path: 'C:/dev/code/web' },
    ],
    statuses: { api: status() },
    errors: {},
    lastFetchAt: {},
    authNeeded: [],
    pins: [],
    lastSelected: null,
    ...overrides,
  };
}

describe('applyRoot', () => {
  it('restores the last selection when the root is genuinely new', () => {
    const store = new RepoStore();

    store.applyRoot(rootView({ lastSelected: 'web' }));

    expect(store.selectedId).toBe('web');
  });

  /* F5 on the folder already open. `open_root` re-sends the same view, and
   * `lastSelected` is whatever was persisted — which is a sweep or a session
   * old. Applying it again would walk the user's selection back to it, so the
   * gate is the difference between a reload and a jump to another repo. */
  it('leaves the in-memory selection alone when the same root is re-sent', () => {
    const store = new RepoStore();
    store.applyRoot(rootView({ lastSelected: 'web' }));
    store.select('api');

    store.applyRoot(rootView({ lastSelected: 'web' }));

    expect(store.selectedId).toBe('api');
  });

  it('drops the selection and the file pane when the root changes', () => {
    const store = new RepoStore();
    store.applyRoot(rootView());
    store.select('api');

    store.applyRoot(rootView({ path: 'C:/dev/other', lastSelected: null }));

    expect(store.selectedId).toBeUndefined();
    expect(store.files).toBeNull();
  });
});

describe('applySweep', () => {
  it('applies results for the root it is showing', () => {
    const store = new RepoStore();
    store.applyRoot(rootView());

    store.applySweep({
      root: 'C:/dev/code',
      statuses: { api: status({ ahead: 3 }) },
      errors: {},
      elapsedMs: 42,
    });

    expect(store.statuses.api.ahead).toBe(3);
    expect(store.lastSweepMs).toBe(42);
  });

  /* A sweep started before File → Open Folder… lands after it. Its statuses
   * are keyed by repo id, and ids from the old root would paint rows that are
   * no longer on screen — or, where the two roots overlap, the right rows with
   * the wrong folder's answers. */
  it('drops results belonging to a root this window has left', () => {
    const store = new RepoStore();
    store.applyRoot(rootView());

    store.applySweep({
      root: 'C:/dev/stale',
      statuses: { api: status({ ahead: 99 }) },
      errors: {},
      elapsedMs: 1,
    });

    expect(store.statuses.api.ahead).toBe(0);
    expect(store.lastSweepMs).toBeNull();
  });
});

describe('applyFetchSweep', () => {
  it('applies fetch times and auth failures for the current root', () => {
    const store = new RepoStore();
    store.applyRoot(rootView());

    store.applyFetchSweep({
      root: 'C:/dev/code',
      lastFetchAt: { api: 1_700_000_000 },
      authNeeded: ['web'],
      elapsedMs: 7,
    });

    expect(store.lastFetchAt.api).toBe(1_700_000_000);
    expect(store.authNeeded.has('web')).toBe(true);
  });

  it('drops a fetch sweep belonging to another root', () => {
    const store = new RepoStore();
    store.applyRoot(rootView());

    store.applyFetchSweep({
      root: 'C:/dev/stale',
      lastFetchAt: { api: 1_700_000_000 },
      authNeeded: ['web'],
      elapsedMs: 7,
    });

    expect(store.lastFetchAt.api).toBeUndefined();
    expect(store.authNeeded.size).toBe(0);
  });
});

describe('applyRepoStatus', () => {
  /* A repo is either readable or it is not, and the row renders the two
   * differently — a status present alongside a stale error would paint a
   * healthy row carrying an error badge. Both directions, because the swap is
   * written as two independent `delete` branches and either can be dropped on
   * its own. */
  it('a fresh status clears the error that repo was carrying', () => {
    const store = new RepoStore();
    store.applyRoot(rootView({ statuses: {}, errors: { api: 'could not read' } }));

    store.applyRepoStatus({
      root: 'C:/dev/code',
      repoId: 'api',
      status: status({ ahead: 1 }),
      error: null,
      files: null,
    });

    expect(store.statuses.api.ahead).toBe(1);
    expect('api' in store.errors).toBe(false);
  });

  it('a fresh error clears the status that repo was carrying', () => {
    const store = new RepoStore();
    store.applyRoot(rootView());

    store.applyRepoStatus({
      root: 'C:/dev/code',
      repoId: 'api',
      status: null,
      error: 'index.lock exists',
      files: null,
    });

    expect(store.errors.api).toBe('index.lock exists');
    expect('api' in store.statuses).toBe(false);
  });

  it('drops an event belonging to another root', () => {
    const store = new RepoStore();
    store.applyRoot(rootView());

    store.applyRepoStatus({
      root: 'C:/dev/stale',
      repoId: 'api',
      status: null,
      error: 'index.lock exists',
      files: null,
    });

    expect('api' in store.errors).toBe(false);
    expect(store.statuses.api.ahead).toBe(0);
  });

  /* The files ride along on the event to save a second `git status` spawn, so
   * they are addressed to whichever repo the *backend* thought was selected.
   * If the selection moved while the event was in flight, painting them would
   * show one repo's rows under another repo's name. */
  it('keeps the carried file list only while it matches the selection', () => {
    const store = new RepoStore();
    store.applyRoot(rootView());
    store.select('api');
    const files = {
      staged: [{ path: 'a.txt', status: 'M' }],
      stagedTotal: 1,
      unstaged: [],
      unstagedTotal: 0,
      conflicted: [],
    };

    store.applyRepoStatus({
      root: 'C:/dev/code',
      repoId: 'web',
      status: status(),
      error: null,
      files,
    });

    expect(store.files).toBeNull();

    store.applyRepoStatus({
      root: 'C:/dev/code',
      repoId: 'api',
      status: status(),
      error: null,
      files,
    });

    expect(store.files?.staged[0].path).toBe('a.txt');
  });
});
