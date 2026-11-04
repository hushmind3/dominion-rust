import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
const apiTarget=process.env.DOMINION_API_URL||'http://127.0.0.1:4317'
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
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
