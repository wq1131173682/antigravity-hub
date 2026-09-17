import daisyui from "daisyui";
import containerQueries from "@tailwindcss/container-queries";

/** @type {import('tailwindcss').Config} */
export default {
    content: [
        "./index.html",
        "./src/**/*.{js,ts,jsx,tsx}",
    ],
    darkMode: 'class',
    theme: {
        extend: {
            // App canvas colour. Defined as a CSS variable in src/App.css so the
            // HTML background, the body background and the Tauri window
            // background all resolve from one place.
            backgroundColor: {
                canvas: 'var(--app-canvas)',
            },
            // Single z-index scale, ordered to clear daisyUI's own layers:
            // daisyUI's `.modal` sets `z-index: 999`, so overlay/dialog must sit
            // above 999. Previously dialogs added `z-[100]`, which won the
            // same-specificity cascade and PULLED the modal DOWN to 100.
            zIndex: {
                base: '0',
                raised: '10',
                sticky: '20',
                overlay: '1000',
                toast: '1100',
                debug: '1200',
            },
            // Entrance animation lives in src/App.css (`.animate-enter`) so it
            // can also be disabled under `prefers-reduced-motion`.
        },
    },
    plugins: [daisyui, containerQueries],

    // NOTE: daisyUI 5 reads its options from a CSS `@plugin "daisyui" { ... }`
    // block, NOT from a `daisyui` key here. The hex theme that used to live in
    // this file was silently ignored for that reason - the bundle always
    // carried daisyUI's stock oklch values (`--color-base-100: oklch(100% 0 0)`
    // in light, `oklch(25.33% .016 252.42)` in dark). Configure themes in
    // src/App.css if the stock palette ever needs to change.
};
