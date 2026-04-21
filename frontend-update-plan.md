# Frontend Update Plan

## Background

This document is a repo-specific upgrade plan for the current frontend in `E:\repo\RsLiteyukiBot`.

Conclusion first:

- The frontend stack **can be upgraded almost completely**, but **not as a single blind dependency bump**.
- The main work is **Vite + TypeScript + HeroUI + Tailwind**.
- `React` and `react-router-dom` are already effectively on recent patch lines through the current lockfile, so they are **not the risky part**.
- `Tailwind 4` is the better long-term target for this project, because the app targets **modern Tauri WebView + modern browsers**, not legacy browsers. But it should only be migrated **after HeroUI is normalized to the stable `2.8.x` line**.
- `HeroUI v3` should **not** be the target for this repo right now. The public docs still position it as release-candidate/beta-era work, which is not the right base for a repo that already has a working dashboard.
- `@tauri-apps/cli` and Rust-side `tauri` crates should be treated as a **separate coordinated step**, not mixed into the main frontend migration.

## Current Stack

Snapshot source:

- `package.json`
- `pnpm-lock.yaml`
- `frontend/vite.config.mts`
- `frontend/tsconfig.json`
- `frontend/tailwind.config.js`
- `frontend/postcss.config.cjs`
- `frontend/src/styles/globals.css`

| Area | Manifest | Current lockfile resolution | Notes |
| --- | --- | --- | --- |
| React | `^19.0.0` | `19.2.5` | Already on current React 19 patch line. |
| React DOM | `^19.0.0` | `19.2.5` | Same as React. |
| React Router DOM | `^7.1.4` | `7.14.1` | Already on recent v7 patch line. |
| HeroUI breadcrumbs | `2.2.7` | `2.2.7` | Version drift exists across HeroUI packages. |
| HeroUI button | `2.2.10` | `2.2.10` |  |
| HeroUI card | `2.2.10` | `2.2.10` |  |
| HeroUI system | `2.4.7` | `2.4.7` |  |
| HeroUI theme | `2.4.6` | `2.4.6` |  |
| HeroUI tooltip | `2.2.8` | `2.2.8` |  |
| Tailwind CSS | `^3.4.17` | `3.4.19` | Tailwind 3 style config is still in use. |
| Vite | `^5.4.8` | `5.4.21` | Simple config, good upgrade candidate. |
| `@vitejs/plugin-react` | `^4.3.4` | `4.7.0` | Config is plain `react()`, low migration risk. |
| TypeScript | `^5.9.2` | `5.9.3` | Current config is modern, but `baseUrl` should be cleaned before TS 6. |
| `@tauri-apps/cli` | `^2.8.4` | `2.10.1` | Frontend CLI already floats to a recent patch line. |
| `motion` | `^12.0.6` | `12.38.0` | No blocking issue found. |
| `react-icons` | `^5.4.0` | `5.6.0` | No blocking issue found. |
| `clsx` | `^2.1.1` | `2.1.1` | Fine as-is. |

Important implication:

- The lockfile already resolves several packages to much newer patch versions than the manifest hints at.
- This means the real upgrade work is **major-line migration**, not patch refresh.

## Recommended Target Matrix

This is the recommended target as of `2026-04-21`.

| Area | Recommended target | Decision |
| --- | --- | --- |
| React / React DOM | Keep on latest stable `19.2.x` line | Upgrade manifest only if you want the manifest to reflect reality. |
| React Router DOM | Keep on latest stable `7.x` line | No urgent code migration required. |
| Vite | `8.x` | Recommended. Simple repo config makes this manageable. |
| `@vitejs/plugin-react` | Latest Vite-8-compatible `6.x` | Recommended. Current config does not rely on removed Babel hooks. |
| TypeScript | `6.0.x` | Recommended, but isolate in its own validation step. |
| HeroUI | All `@heroui/*` packages unified to stable `2.8.x` | Required before Tailwind 4. |
| Tailwind CSS | `4.2.x` | Recommended, but only after HeroUI normalization. |
| Tailwind integration | `@tailwindcss/vite` | Recommended. This repo does not need to keep PostCSS for Tailwind. |
| `@tauri-apps/cli` | Keep on latest `2.x` patch only after coordinated validation | Do later, not in the main frontend pass. |
| HeroUI v3 | Do not target now | Public docs still show release-candidate / beta status. |

## Official Reference Snapshot

These references support the target decisions above:

- React 19.2 blog: https://react.dev/blog/2025/10/01/react-19-2
- React Router latest v7 release line: https://github.com/remix-run/react-router/releases
- Vite migration guide: https://vite.dev/guide/migration.html
- Vite 8 release line: https://github.com/vitejs/vite/releases
- Vite Node requirement guide: https://vite.dev/guide/
- Tailwind CSS v4 announcement: https://tailwindcss.com/blog/tailwindcss-v4
- Tailwind upgrade guide: https://tailwindcss.com/docs/upgrade-guide
- HeroUI Tailwind v4 guide: https://www.heroui.com/docs/guide/tailwind-v4
- HeroUI installation guide: https://www.heroui.com/docs/guide/installation
- HeroUI release feed: https://github.com/heroui-inc/heroui/releases
- HeroUI v3 docs: https://v3.heroui.com/docs
- TypeScript 6.0 announcement: https://devblogs.microsoft.com/typescript/announcing-typescript-6-0/
- Tauri release feed: https://github.com/tauri-apps/tauri/releases

## Feasibility Summary

### Can Upgrade Now

- `Vite 5 -> 8`
- `@vitejs/plugin-react 4 -> 6`
- `TypeScript 5.9 -> 6.0`
- All `@heroui/*` packages to unified stable `2.8.x`
- `Tailwind 3 -> 4`
- Tailwind integration from `PostCSS` to `@tailwindcss/vite`

### Already Effectively Current Enough

- `react`
- `react-dom`
- `react-router-dom`
- `motion`
- `react-icons`
- `clsx`

These still deserve a lockfile refresh during the migration, but they are not the risky part of the plan.

### Do Not Upgrade In The Main Pass

- `HeroUI v3`
- Tauri Rust crates as part of the same PR
- React Router package-entry refactor (`react-router-dom` -> `react-router` / `react-router/dom`)

The first is too unstable for this repo. The second couples frontend and Rust packaging. The third is optional cleanup, not required for a working upgrade.

## Repo-Specific Risk Inventory

### 1. Node Runtime Requirement For Vite 8

Vite 8 requires a newer Node baseline than older Vite 5 setups. Before starting the migration, verify the runtime actually used by `pnpm`, not only `node -v`.

Current local observation:

- `node -v` reports `v25.8.1`
- sandboxed `pnpm` stack traces still report `Node.js v20.11.1`

Action:

- Verify `pnpm exec node -v` before attempting the Vite 8 upgrade.
- If `pnpm` still runs on `20.11.x`, fix that first. Vite 8 expects `20.19+` or `22.12+`.

### 2. Tailwind 4 Is The Main Compatibility Surface

Current repo facts:

- `frontend/postcss.config.cjs` is still Tailwind 3 style.
- `frontend/src/styles/globals.css` still uses:
  - `@tailwind base;`
  - `@tailwind components;`
  - `@tailwind utilities;`
- `frontend/tailwind.config.js` contains a `safelist`, and Tailwind 4 no longer supports `safelist` from JS config.
- `frontend/tailwind.config.js` also points Tailwind 3 `content` at `../node_modules/@heroui/theme/dist/**/*`.

Observed Tailwind-4-sensitive usage in this repo:

- `shadow-sm`
- `backdrop-blur-sm`
- `flex-shrink-0`
- `outline-none`
- `space-y-*`
- default `border` / `ring` expectations in custom inputs and glass panels

The good news is that the current `safelist` looks removable. The theme color utilities it tried to preserve are already used statically in `frontend/src`, so this is probably legacy safety padding rather than an active requirement.

### 3. HeroUI Version Drift

The project currently mixes `2.2.x` and `2.4.x` HeroUI packages. That is manageable today, but it is the wrong base for a Tailwind 4 migration.

This repo should first normalize HeroUI to one stable line:

- keep split packages for now
- move every `@heroui/*` dependency to the same `2.8.x` patch line
- do **not** switch to `HeroUI v3` in the same pass

### 4. TypeScript 6 Will Surface Small Config Debt

`frontend/tsconfig.json` currently includes:

- `"moduleResolution": "Bundler"` which is good
- `"baseUrl": "."` which is not ideal going into TS 6

Planned cleanup:

- remove `"baseUrl": "."`
- change `"@/*": ["src/*"]` to `"@/*": ["./src/*"]`

### 5. Tauri Is Not Frontend-Isolated

`package.json` and `src-tauri/Cargo.toml` both participate in the Tauri toolchain. Even if the web UI compiles, a Tauri upgrade still needs:

- `pnpm build`
- `cargo check --manifest-path src-tauri/Cargo.toml --locked --offline`
- a real `tauri dev` smoke pass

So it should be a separate stage.

## Recommended Upgrade Order

### Phase 0. Freeze Baseline

Goal:

- Capture a known-good baseline before changing anything.

Checklist:

1. Run `git status --short`.
2. Run `pnpm build`.
3. Run `cargo check --manifest-path src-tauri/Cargo.toml --locked --offline`.
4. Run `pnpm exec node -v`.
5. Capture screenshots for these routes:
   - `/`
   - `/runtime`
   - `/adapters`
   - `/llm`
   - `/commands`
   - `/plugins`
   - `/diagnostics`

Ship gate:

- Do not start the migration if baseline build is not green.

### Phase 1. Upgrade Build Toolchain Without Touching Tailwind

Goal:

- Isolate bundler and type-system upgrades first.

Package changes:

- `vite -> ^8`
- `@vitejs/plugin-react -> ^6`
- `typescript -> ^6.0.0`

React-line changes:

- Optionally normalize the manifest to the already effective lockfile line:
  - `react -> ^19.2.0`
  - `react-dom -> ^19.2.0`
  - `react-router-dom -> ^7.14.0`

Expected code changes:

- `frontend/tsconfig.json`
  - remove `baseUrl`
  - make `paths` explicit with `./src/*`
- possibly `frontend/vite.config.mts`
  - only if Vite 8 warns about config shape

Validation:

- `pnpm build`
- manual route smoke in browser

Stop point:

- If Vite 8 or TS 6 surfaces unrelated type/runtime issues, fix them here before touching HeroUI or Tailwind.

### Phase 2. Normalize HeroUI To Stable 2.8.x

Goal:

- Get the component library onto a Tailwind-4-capable stable line before migrating the CSS pipeline.

Package changes:

- `@heroui/breadcrumbs -> ^2.8`
- `@heroui/button -> ^2.8`
- `@heroui/card -> ^2.8`
- `@heroui/system -> ^2.8`
- `@heroui/theme -> ^2.8`
- `@heroui/tooltip -> ^2.8`

Rules:

- Keep the current split-package import structure.
- Do not switch to `@heroui/react` in the same pass.
- Do not target HeroUI v3.

Potential support file:

- `.npmrc`
  - add the HeroUI docs' recommended `pnpm` hoist configuration only if install resolution becomes unstable

Validation:

- `pnpm build`
- visual smoke for cards, buttons, breadcrumbs, tooltip surfaces, dark mode

Stop point:

- If HeroUI `2.8.x` introduces component-level regressions under Tailwind 3, fix them before touching Tailwind 4.

### Phase 3. Migrate Tailwind 3 To Tailwind 4

Goal:

- Move the project to Tailwind 4 with the least repo-specific churn.

Recommended integration strategy:

- Use `@tailwindcss/vite`
- remove the old Tailwind PostCSS path

Package changes:

- `tailwindcss -> ^4`
- add `@tailwindcss/vite -> ^4`
- remove `postcss`
- remove `autoprefixer`

Config changes:

- `frontend/vite.config.mts`
  - add Tailwind Vite plugin
- delete `frontend/postcss.config.cjs`
- `frontend/src/styles/globals.css`
  - replace old `@tailwind` directives with `@import "tailwindcss";`
  - add `@config "../../tailwind.config.js";` as a compatibility bridge
  - add explicit `@source` entries if HeroUI theme files are not auto-detected
- `frontend/tailwind.config.js`
  - remove `safelist`
  - keep theme extension and HeroUI plugin for the first migration pass

Important Tailwind 4 class rewrites in this repo:

- `shadow-sm -> shadow-xs`
- `backdrop-blur-sm -> backdrop-blur-xs`
- `flex-shrink-0 -> shrink-0`
- `outline-none -> outline-hidden`

Manual review required for:

- `space-y-*` because Tailwind 4 changed the selector model
- components that depend on implicit border color
- components that depend on implicit ring width/color

Recommended file sweep:

- `frontend/src/App.tsx`
- `frontend/src/components/sidebar/menus.tsx`
- `frontend/src/components/sidebar/index.tsx`
- `frontend/src/components/dashboard/display_network_item.tsx`
- `frontend/src/components/dashboard/runtime_identity_card.tsx`
- `frontend/src/components/chrome/search_field.tsx`
- `frontend/src/components/chrome/page_header.tsx`
- `frontend/src/components/dashboard/section_surface.tsx`
- `frontend/src/pages/adapters/index.tsx`
- `frontend/src/pages/commands/index.tsx`
- `frontend/src/pages/diagnostics/index.tsx`
- `frontend/src/pages/llm/index.tsx`
- `frontend/src/pages/plugins/index.tsx`
- `frontend/src/pages/runtime/index.tsx`
- `frontend/src/pages/overview/index.tsx`

Pragmatic rule:

- Do **not** try to fully rewrite the theme into Tailwind 4 CSS-first style in the first pass.
- First get a working Tailwind 4 build using `@config` compatibility.
- Only after the repo is stable should you consider moving theme tokens out of `tailwind.config.js` into CSS `@theme`.

Validation:

- `pnpm build`
- side-by-side screenshot comparison against Phase 0
- dark / light mode toggle
- button, card, breadcrumb, tooltip rendering
- search input focus ring and border behavior

Rollback checkpoint:

- If Tailwind 4 visual regressions are broader than expected, stop here and ship only Phases 1-2.

### Phase 4. Optional Cleanup After Tailwind 4 Is Stable

Goal:

- Clean up non-blocking modernization items after the main migration is already working.

Optional work:

- evaluate moving some `react-router-dom` imports to `react-router` / `react-router/dom`
- reduce leftover compatibility code in `tailwind.config.js`
- move custom theme tokens into Tailwind 4 CSS syntax if that simplifies maintenance

This is explicitly optional. It is not required for a successful stack upgrade.

### Phase 5. Optional Tauri Patch Alignment

Goal:

- Align frontend CLI and Rust-side Tauri packages after the web UI migration is already stable.

Scope:

- `package.json`
- `src-tauri/Cargo.toml`
- `Cargo.lock`
- `src-tauri/tauri.conf.json`

Rule:

- Do not combine this with the Tailwind migration PR.

Validation:

- `pnpm build`
- `cargo check --manifest-path src-tauri/Cargo.toml --locked --offline`
- `tauri dev` smoke test

## Detailed File Change Plan

| File | Planned change | Why |
| --- | --- | --- |
| `package.json` | bump Vite / plugin-react / TypeScript / HeroUI / Tailwind related dependencies; remove old PostCSS-only Tailwind deps | Main dependency alignment point |
| `pnpm-lock.yaml` | regenerate after each upgrade phase | Needed because current lockfile already resolves much newer patches |
| `.npmrc` | add HeroUI-recommended pnpm hoist config only if needed | Prevent HeroUI package resolution issues under pnpm |
| `frontend/vite.config.mts` | add Tailwind Vite plugin; keep existing alias/proxy/server structure | Simplest Tailwind 4 integration path |
| `frontend/postcss.config.cjs` | delete | No longer needed once Tailwind uses Vite plugin |
| `frontend/tailwind.config.js` | keep temporarily, remove `safelist`, keep theme/plugin config | Lowest-risk bridge into Tailwind 4 |
| `frontend/src/styles/globals.css` | replace Tailwind 3 directives, add `@config`, possibly add `@source`, keep custom component layer | Central CSS migration point |
| `frontend/tsconfig.json` | remove `baseUrl`, keep bundler resolution, update path mapping | TS 6 readiness |
| `frontend/src/App.tsx` | replace `backdrop-blur-sm` | Tailwind 4 utility rename |
| `frontend/src/components/sidebar/menus.tsx` | replace `shadow-sm` | Tailwind 4 utility rename |
| `frontend/src/components/sidebar/index.tsx` | replace `shadow-sm` / `backdrop-blur-sm`; review `space-y-*` | Tailwind 4 rename + selector review |
| `frontend/src/components/dashboard/display_network_item.tsx` | replace `flex-shrink-0` | Tailwind 4 utility rename |
| `frontend/src/components/dashboard/runtime_identity_card.tsx` | replace `flex-shrink-0` | Tailwind 4 utility rename |
| `frontend/src/components/chrome/search_field.tsx` | replace `outline-none`; verify border/ring visuals | Tailwind 4 behavior changes hit this file directly |
| `frontend/src/components/chrome/page_header.tsx` and several `pages/*/index.tsx` files | verify `space-y-*` layout | Tailwind 4 changed space-between selector behavior |

## Command Checklist

Run these in order.

### Baseline

```powershell
git status --short
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml --locked --offline
pnpm exec node -v
```

### Phase 1

```powershell
pnpm up react@^19.2 react-dom@^19.2 react-router-dom@^7.14 motion@latest react-icons@latest clsx@latest
pnpm up -D vite@^8 @vitejs/plugin-react@^6 typescript@^6.0.0
pnpm build
```

### Phase 2

```powershell
pnpm up @heroui/breadcrumbs@^2.8 @heroui/button@^2.8 @heroui/card@^2.8 @heroui/system@^2.8 @heroui/theme@^2.8 @heroui/tooltip@^2.8
pnpm build
```

### Phase 3

```powershell
pnpm remove postcss autoprefixer
pnpm add -D tailwindcss@^4 @tailwindcss/vite@^4
pnpm build
```

Optional helper on a throwaway branch only:

```powershell
pnpm dlx @tailwindcss/upgrade
```

Do not trust the upgrade tool as the final migration for this repo. Use it only to generate a diff hint, because this project has:

- HeroUI plugin wiring
- custom theme colors
- Tailwind 3 `safelist`
- custom layered CSS

### Phase 5

```powershell
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml --locked --offline
```

## Validation Gates

Each phase must pass all relevant gates before moving to the next phase.

### Build Gates

- `pnpm build`
- `cargo check --manifest-path src-tauri/Cargo.toml --locked --offline` after any Tauri-related change

### Visual Gates

- overview page still renders
- sidebar still expands/collapses correctly
- cards still keep glassmorphism and shadow hierarchy
- dark mode still works
- search input focus ring still looks correct
- no missing themed color utilities
- no spacing regressions on stacked sections

### Runtime Gates

- route navigation still works with `HashRouter`
- `HeroUIProvider` navigation integration still works
- Tauri dev shell still opens after any Tauri step

## Rollback Strategy

Recommended commit split:

1. `chore(frontend): upgrade vite and typescript`
2. `chore(frontend): normalize heroui to 2.8`
3. `chore(frontend): migrate tailwind 3 to 4`
4. `chore(tauri): align tauri frontend and rust toolchain` optional

Rollback rules:

- If Phase 1 fails badly, roll back only the toolchain commit.
- If Phase 2 fails badly, keep Phase 1 and roll back HeroUI only.
- If Phase 3 fails visually, keep Phases 1-2 and postpone Tailwind 4.
- Do not roll back unrelated Rust or repo-root changes.

The safest partial-ship checkpoint is:

- `Vite 8 + TypeScript 6 + HeroUI 2.8.x`, while still staying on Tailwind 3

That checkpoint is still valuable even if Tailwind 4 is deferred.

## Final Recommendation

Recommended implementation path:

1. Upgrade `Vite`, `@vitejs/plugin-react`, and `TypeScript` first.
2. Normalize all HeroUI packages to stable `2.8.x`.
3. Migrate `Tailwind 3 -> 4` using `@tailwindcss/vite`, not the old PostCSS path.
4. Keep `tailwind.config.js` as a temporary bridge, but remove `safelist`.
5. Defer Tauri patch alignment to a separate final step.

In other words:

- **Yes**, this frontend stack is upgradeable.
- **No**, it should not be upgraded in one blind PR.
- The correct strategy is **three staged frontend migrations, then one optional Tauri sync step**.
