 # Repository Guidelines
 
 ## Project Structure & Module Organization
 
 ```
 .
 ├── src/                  # Frontend (React 19 + TypeScript)
 │   ├── components/       # UI components grouped by feature
 │   │   ├── accounts/     # Account management components
 │   │   ├── common/       # Shared/reusable components
 │   │   ├── dashboard/    # Dashboard widgets
 │   │   ├── debug/        # Debug console components
 │   │   ├── layout/       # App shell and layout
 │   │   └── navbar/       # Top navigation bar
 │   ├── config/           # App configuration (e.g., model config)
 │   ├── hooks/            # Custom React hooks
 │   ├── locales/          # i18n translation files (12 languages)
 │   ├── pages/            # Top-level route pages
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
 ├── docker/               # Docker compose and Dockerfiles
 ├── docs/                 # Internal documentation
 ├── workflows/            # GitHub Actions CI/CD
 ├── scripts/              # Build and release helper scripts
 ├── public/               # Static assets (fonts, icons)
 └── dist/                 # Built frontend output (gitignored)
 ```
 
 The frontend follows a feature-grouped component structure. Each store in `src/stores/` owns a single domain (accounts, config, platforms, view state). The Rust backend mirrors this with `models/` for data structures and `modules/` for business logic.
 
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
 - Components use PascalCase, files use camelCase (e.g., `accountService.ts`, `useAccountStore.ts`).
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
 
 - API keys are stored locally via the Tauri filesystem plugin. The proxy runs on `127.0.0.1:8045` and only listens locally.
 - The Vite dev server proxies `/api/` requests to the backend; no external exposure in production.
 - The Tauri CSP is restrictive: `default-src 'self'` with minimal allowances for images and styles. Do not relax it without review.
 - Sensitive configuration (signing keys, API tokens) must never be committed. Use environment variables or Tauri build secrets.
 - Docker configurations in `docker/` are intended for backend services only — the Tauri desktop app is not containerized.
