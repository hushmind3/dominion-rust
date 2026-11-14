import { defineConfig, searchForWorkspaceRoot } from 'vite'
import { fileURLToPath } from 'node:url'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
const webRoot = fileURLToPath(new URL('.', import.meta.url))
const sdkRoot = fileURLToPath(new URL('../sdk/typescript/src/', import.meta.url))
const apiTarget=process.env.DOMINION_API_URL||'http://127.0.0.1:4317'
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    // The frontend dogfoods the SDK source outside the web directory.
    fs: {allow: [searchForWorkspaceRoot(webRoot), sdkRoot]},
    host: '127.0.0.1', port: 5173, strictPort: true,
    proxy: { '/api': {
      target: apiTarget, changeOrigin: true,
      configure(proxy) {
        proxy.on('proxyReq', (request, incoming) => {
          // Only this loopback Vite origin may stand in for the Rust origin.
          if (['http://127.0.0.1:5173', 'http://localhost:5173'].includes(incoming.headers.origin || '')) {
            request.setHeader('origin', new URL(apiTarget).origin)
          }
        })
      },
    } },
  },
  build: { outDir: 'dist' }
})
