import { defineConfig } from 'vitest/config'
import path from 'path'
import { fileURLToPath } from 'url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

// Only `@quillmark-wasm/runtime` names a public entry point. The other aliases
// reach the internal build artifacts directly — the Typst backend binary, whose
// API is a superset, and the Typst-less core build — so the bundle suites can
// exercise builds no consumer can import.
export const WORKSPACE_ROOT = path.resolve(__dirname, '..', '..', '..')
export const WASM_BUNDLE_PATH = path.join(WORKSPACE_ROOT, 'pkg', 'backends', 'typst', 'wasm.js')
export const WASM_PDFFORM_BUNDLE_PATH = path.join(WORKSPACE_ROOT, 'pkg', 'backends', 'pdfform', 'wasm.js')
export const WASM_CORE_BUNDLE_PATH = path.join(WORKSPACE_ROOT, 'pkg', 'core', 'wasm.js')
export const WASM_RUNTIME_BUNDLE_PATH = path.join(WORKSPACE_ROOT, 'pkg', 'runtime', 'runtime.js')

export default defineConfig({
  // No wasm plugin: the builds are `--target web`, so nothing imports a `.wasm`
  // module and nothing emits top-level await. The suites instantiate through
  // the runtime's `init` (or `initSync` on the generated builds they drive
  // directly), as a consumer does. A plugin here would mask that.
  resolve: {
    // `pkg/runtime/runtime.js` resolves the wasm byte source through the
    // `#quillmark-env` subpath import; pin the Node half, since these suites
    // run under Node and the browser half's `fetch` rejects `file:` URLs.
    conditions: ['node'],
    alias: {
      // More specific first: the alias match is `find` followed by `/` or end,
      // so `@quillmark-wasm/{core,runtime,pdfform}` must precede the `@quillmark-wasm` prefix.
      '@quillmark-wasm/runtime': WASM_RUNTIME_BUNDLE_PATH,
      '@quillmark-wasm/pdfform': WASM_PDFFORM_BUNDLE_PATH,
      '@quillmark-wasm/core': WASM_CORE_BUNDLE_PATH,
      '@quillmark-wasm': WASM_BUNDLE_PATH,
    },
  },
  test: {
    environment: 'node',
    testTimeout: 40000,
  },
})
