import { defineConfig, devices } from 'playwright/test'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import os from 'node:os'
import fs from 'node:fs'

const webRoot=path.dirname(fileURLToPath(import.meta.url))
const repoRoot=path.resolve(webRoot,'..')
const runtime=fs.mkdtempSync(path.join(os.tmpdir(),'dominion-browser-'))
export default defineConfig({
  testDir:'./e2e',
  testMatch:'*.spec.mjs',
  timeout:60000,
  expect:{timeout:30000},
  retries:process.env.CI?1:0,
  workers:1,
  reporter:process.env.CI?[['line'],['html',{open:'never',outputFolder:'playwright-report'}]]:'list',
  use:{baseURL:'http://127.0.0.1:5173',trace:'retain-on-failure',
    screenshot:'only-on-failure',...devices['Desktop Chrome']},
  webServer:[
    {command:'node e2e/mock-provider.mjs',cwd:webRoot,
      url:'http://127.0.0.1:18991/health',reuseExistingServer:!process.env.CI,
      timeout:15000},
    {command:'cargo run --quiet',cwd:repoRoot,
      url:'http://127.0.0.1:8787/api/v1/health',
      env:{...process.env,DOMINION_BIND:'127.0.0.1:8787',
        DOMINION_BASE_URL:'http://127.0.0.1:18991',
        DOMINION_ECONOMY_BASE_URL:'http://127.0.0.1:18991',
        DOMINION_MODEL:'mock-benchmark',
        DOMINION_ECONOMY_MODEL:'mock-benchmark',
        DOMINION_DATA_DIR:path.join(runtime,'data'),
        DOMINION_WORKSPACE_DIR:path.join(runtime,'workspace'),
        DOMINION_MCP_SERVERS:'[]',
        DOMINION_ACCESS_TOKEN:''
      },reuseExistingServer:!process.env.CI,timeout:200000},
    {command:'npm run dev -- --host 127.0.0.1',cwd:webRoot,
      url:'http://127.0.0.1:5173',reuseExistingServer:!process.env.CI,
      timeout:45000},
  ],
})
