import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
const apiTarget=process.env.DOMINION_API_URL||'http://127.0.0.1:4317'
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: { host: '127.0.0.1', port: 5173, proxy: { '/api': apiTarget } },
  build: { outDir: 'dist' }
})
