# twogit

A fast dashboard over many local git repositories. Built for the case where you have
dozens of repos in one folder, roughly five are active at a time, and you want to know at a
glance which ones need attention.

Four verbs — fetch, pull, commit, push — plus staging and branch switching. Not a general
git client. See **[SPEC.md](SPEC.md)** for the full design.

**Status: build step 1 of 10** — shell, three resizable panes, dark token layer, settings
persistence. The panes are placeholders; discovery and status land in step 2.

---

## Prerequisites

| | Status on this machine | Install |
| --- | --- | --- |
| Node 22+ | ✅ present | — |
| Git 2.37+ | ✅ 2.53 | — |
| WebView2 runtime | ✅ present | — |
| Rust (stable) | ❌ missing | `winget install Rustlang.Rustup` |
| MSVC C++ build tools | ❌ missing | Visual Studio Installer → Modify → **Desktop development with C++** |

Rust's default Windows toolchain (`x86_64-pc-windows-msvc`) links with MSVC, which is why
the C++ workload is required even though no C++ is written here. Visual Studio 2026 is
already installed — the workload just isn't selected.

## Running

```sh
npm install
npm run tauri:dev      # the real app — needs Rust
```

The frontend also runs standalone in a browser, which is useful for layout work and needs
no Rust at all:

```sh
npm run dev            # http://localhost:1420
```

Outside Tauri, settings fall back to `localStorage` instead of the Rust backend, so pane
widths still persist.

## Checks

```sh
npm run check          # svelte-check + TypeScript
npm run build          # check, then production bundle
cargo test --manifest-path src-tauri/Cargo.toml
```

## Layout

```
src/
  app.css                    token layer — every colour lives here (SPEC §11)
  App.svelte                 pane grid, drag geometry, minimum widths
  lib/
    settings.svelte.ts       settings mirror; Tauri IPC or localStorage
    Divider.svelte           draggable pane separator
    EmptyState.svelte
    panes/
      Pane.svelte            shared header + scrolling body
      RepoList.svelte        left    — step 2
      CommitPane.svelte      middle  — steps 4 and 7
      GraphPane.svelte       right   — step 6
src-tauri/
  src/
    main.rs                  desktop entry point
    lib.rs                   app state, Tauri commands
    settings.rs              versioned, atomically-written settings
scripts/
  make-icon.mjs              regenerates the placeholder icon source
```

## Notes

- **Pane widths are stored as fractions**, not pixels, so they survive window resizing.
  Minimum widths win over the stored fraction when the window is too narrow; the middle
  pane yields first, then the left.
- **Rust owns application state.** The frontend is a view. This is what keeps multi-window
  safe later — per-repo write queues and the global git semaphore are process-wide, so
  twogit runs as one process with many windows rather than many processes (SPEC §9.2).
- **Settings are advisory.** A corrupt or unreadable file resets to defaults and logs a
  warning; it never blocks startup.
- The icon is a placeholder. `node scripts/make-icon.mjs` regenerates the 512×512 source,
  `npm run icon` expands it into the bundled set.
