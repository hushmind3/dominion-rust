import {defineConfig,devices} from 'playwright/test'

/** Exercise the real Rust HTTP adapter behind the unmodified React/Vite UI. */
export default defineConfig({
 testDir:'./e2e',
 fullyParallel:false,
 retries:process.env.CI?1:0,
 workers:1,
 reporter:process.env.CI?'github':'list',
 use:{baseURL:'http://127.0.0.1:5173',trace:'retain-on-failure',...devices['Desktop Chrome']},
 webServer:[
  {
   command:'cargo run --manifest-path ../Cargo.toml -- --home ..',
   url:'http://127.0.0.1:4317/api/health',
   timeout:360_000,
   reuseExistingServer:!process.env.CI,
   env:{
    DOMINION_BIND:'127.0.0.1:4317',
    DOMINION_DATA_DIR:'./runtime/e2e',
    DOMINION_WORKSPACE_DIR:'./workspace/e2e',
    DOMINION_ACCESS_TOKEN:'',
    // CI proves that a pre-registered product OAuth application enables
    // account login with no end-user credential-entry screen.
    DOMINION_GOOGLE_OAUTH_CLIENT_ID:'dominion-e2e-google-app',
    DOMINION_GOOGLE_OAUTH_CLIENT_SECRET:'e2e-server-secret-never-expose',
    DOMINION_GITHUB_OAUTH_CLIENT_ID:'dominion-e2e-github-app',
    DOMINION_GITHUB_OAUTH_CLIENT_SECRET:'e2e-server-secret-never-expose'
   },
  },
  {
   command:'npm run dev -- --port 5173 --strictPort',
   url:'http://127.0.0.1:5173',
   timeout:90_000,
   reuseExistingServer:!process.env.CI,
  }
 ],
})
