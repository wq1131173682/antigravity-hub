 # Repository Guidelines
 
 ## Project Structure & Module Organization
 
 ```
 .
 ├── src/                  # Frontend (React 19 + TypeScript)
 │   ├── components/       # UI components grouped by feature
 │   │   ├── common/       # Shared/reusable components
 │   │   ├── dashboard/    # Dashboard widgets
 │   │   ├── layout/       # App shell, title bar, mini view
 │   │   └── navbar/       # Top navigation bar
 │   ├── config/           # App configuration (e.g., model config)
 │   ├── locales/          # i18n translation files (12 languages)
 │   ├── pages/            # Top-level route pages (Dashboard, Accounts, Settings)
 │   ├── services/         # API abstraction layer (backend calls)
 │   ├── stores/           # Zustand state stores
 │   ├── types/            # TypeScript type definitions
 │   └── utils/            # Utility functions
 ├── src-tauri/            # Backend (Rust + Tauri v2)
 │   └── src/
 │       ├── commands/     # Tauri IPC commands
 │       ├── models/       # Data models (apikey, config, platform, etc.)
 │       ├── modules/      # Core business logic (proxy, keystore, scheduler)
 │       └── utils/        # Shared utilities (HTTP client, etc.)
 ├── docs/                 # Internal documentation
 ├── scripts/              # Local dev helpers (gitignored)
 ├── public/               # Static assets (Effra font, app icon)
 └── dist/                 # Built frontend output (gitignored)
 ```
 
 The frontend follows a feature-grouped component structure. Each store in `src/stores/` owns a single domain (platforms, config, view state). The Rust backend mirrors this with `models/` for data structures and `modules/` for business logic.

 This repo is a **derivative work** of [`lbjlaq/Antigravity-Manager`](https://github.com/lbjlaq/Antigravity-Manager)
 (CC BY-NC-SA 4.0) rebuilt as a key-rotation proxy. It shares **no git history**
 with upstream, so upstream changes must be ported by hand. See `DESIGN.md`
 section 7. Only three routes exist: `/` (Dashboard), `/accounts`, `/settings`.
 
 ## Build, Test, and Development Commands
 
 | Command | Description |
 |---|---|
 | `npm run dev` | Start Vite dev server (port 1420) |
 | `npm run build` | Type-check and build the frontend |
 | `npm run preview` | Preview the production build |
 | `npm run tauri dev` | Run the full Tauri app in development mode |
 | `npm run tauri build` | Build a distributable Tauri app bundle |
 | `npm run tauri:debug` | Run Tauri dev with Rust debug logging |
 | `npx tsc --noEmit` | Type-check the frontend without emitting files |
 
 For Rust-specific checks:
 
 ```bash
 cd src-tauri
 cargo check           # Check compilation
 cargo clippy -- -D warnings   # Lint with strict rules
 cargo fmt -- --check  # Verify formatting
 ```
 
 ## Coding Style & Naming Conventions
 
 **Frontend (TypeScript/React)**
 
 - Indentation: 2 spaces. Use single quotes for strings.
 - Components use PascalCase, files use camelCase (e.g., `platformService.ts`, `usePlatformStore.ts`).
 - React components are `.tsx` files; pure logic lives in `.ts` files.
 - TypeScript is strict mode: `noUnusedLocals` and `noUnusedParameters` are enforced.
 - CSS uses Tailwind utility classes with daisyUI theme tokens. PostCSS + Autoprefixer handle vendor prefixes.
 - State management uses Zustand with a single store per domain.
 
 **Backend (Rust)**
 
 - Indentation: 4 spaces. Follow the standard Rust style guide (`cargo fmt`).
 - Naming uses snake_case for functions and variables, PascalCase for types and structs.
 - All public APIs should include `#[derive(Debug, Clone, Serialize, Deserialize)]` where applicable.
 - `cargo clippy` with `-D warnings` is enforced in CI — no warnings allowed.
 
 ## Testing Guidelines
 
 This project does not currently include a test suite. When adding tests:
 - Use a standard React testing framework (Vitest is recommended, matching the Vite toolchain).
 - Place test files co-located with their source files as `*.test.ts` or `*.test.tsx`.
 - For Rust, use `#[cfg(test)]` modules within each module file, following standard Rust conventions.
 - CI runs `cargo check` and `cargo clippy` on every push to `main` and `master`.
 
 ## Commit & Pull Request Guidelines
 
 **Commit messages**
 
 - Use the [Conventional Commits](https://www.conventionalcommits.org/) format: `type(scope): description`.
 - Common types: `feat`, `fix`, `refactor`, `chore`, `docs`, `style`, `i18n`.
 - Keep the subject line under 72 characters.
 
 **Pull requests**
 
 - Provide a clear description of the change and its motivation.
 - Link any related issues.
 - Include screenshots for UI changes (before/after where relevant).
 - Ensure the CI pipeline passes (TypeScript check, Rust check, Tauri build).
 - For cross-platform changes, note which platforms were tested.
 
 ## Security & Configuration Tips
 
 - API keys are stored locally by the Rust backend in `api_keys.json` under the
   app data dir (`src-tauri/src/modules/keystore.rs` uses `std::fs` directly).
   The proxy listens on `127.0.0.1:8045` and is local-only by default.
 - The Vite dev server proxies `/api/` requests to the backend; no external exposure in production.
 - The Tauri CSP is restrictive: `default-src 'self'` with minimal allowances for images and styles. Do not relax it without review.
 - Sensitive configuration (signing keys, API tokens) must never be committed. Use environment variables or Tauri build secrets.
 - `@tauri-apps/plugin-*` npm bindings are NOT used by the frontend. The Rust
   side registers `tauri-plugin-dialog`, `-fs`, `-opener` and `-updater` in
   `lib.rs`, but no frontend module imports their JS counterparts. The one
   exception historically was the updater, which the UI drives through the
   custom `check_for_updates` / `install_update` IPC commands instead.
