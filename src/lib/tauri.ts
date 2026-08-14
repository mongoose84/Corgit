/**
 * `npm run dev` in a plain browser has no Rust backend. Everything that talks
 * to it degrades rather than throwing, so the layout stays workable without the
 * toolchain — but nothing may pretend to have git data it cannot have.
 */
export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
