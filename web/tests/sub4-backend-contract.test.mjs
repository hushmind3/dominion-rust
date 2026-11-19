// The main React shell and independent Sub4 Rust backend have different
// internal modules. Guard real public API bridges without faking responses.
import {test} from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
const source=relative=>readFileSync(new URL(relative,import.meta.url),'utf8')

test('main Rooms frontend uses cancellable HTTP admission and native progression',()=>{
 const ui=source('../src/pages/Rooms.tsx')
 const backend=source('../../src/frontend_rooms.rs')
 const groups=source('../../src/group.rs')
 assert.match(ui,/\/api\/rooms\/'\+room\.id\+'\/messages\/background/)
 assert.match(backend,/\.route\("\/rooms\/\{id\}\/messages\/background",post\(send_background\)\)/)
 assert.match(backend,/frontend_turn_active\(id\)/)
 assert.match(groups,/pub fn try_frontend_turn\(/)
})
test('main public model browsing needs no credential and shares a backend snapshot',()=>{
 const ui=source('../src/components/DominionCloudAccount.tsx')
 const backend=source('../../src/frontend_providers.rs')
 const directory=source('../../src/frontend_provider_public.rs')
 assert.match(ui,/\/api\/providers\/catalog\//)
 assert.match(backend,/\/providers\/catalog\/\{id\}\/models/)
 assert.match(backend,/\/providers\/openrouter\/free-models/)
 assert.match(directory,/static MODELS_DEV_DOCUMENT:OnceLock/)
 assert.match(directory,/load_models_dev_document/)
 assert.match(directory,/public_models_for_endpoint/)
})
test('main GitHub connect and disconnect map to native encrypted-vault routes',()=>{
 const ui=source('../src/components/ConnectedAccounts.tsx')
 const login=source('../../src/github_cli_login.rs')
 const oauth=source('../../src/frontend_oauth.rs')
 assert.match(ui,/\/api\/providers\/cli\/github\/login/)
 assert.match(login,/\/providers\/cli\/github\/login/)
 assert.match(login,/import_github_cli/)
 assert.match(oauth,/\/oauth\/\{id\}/)
 assert.match(oauth,/store\.accounts\.remove\(&id\)/)
})
test('Hugging Face browsing has a real safe Rust read-only metadata adapter',()=>{
 const ui=source('../src/components/ModelExplorer.tsx')
 const routes=source('../../src/frontend_huggingface.rs')
 const runtime=source('../../src/frontend_local_models.rs')
 assert.match(ui,/\/api\/model-library\/catalog\?/)
 assert.match(ui,/\/api\/model-library\/details\?id=/)
 assert.match(routes,/\.route\("\/model-library\/catalog",get\(catalog\)\)/)
 assert.match(routes,/\.route\("\/model-library\/details",get\(details\)\)/)
 assert.match(routes,/huggingface\.co\/api\/models/)
 assert.match(runtime,/crate::frontend_huggingface::routes\(\)/)
})
test('retired Money backend, scheduler and screen are absent',()=>{
 const lib=source('../../src/lib.rs')
 const sdk=source('../../src/sdk.rs')
 const api=source('../../src/api.rs')
 const adapter=source('../../src/frontend_api.rs')
 const app=source('../src/App.tsx')
 for(const file of [lib,sdk,api,adapter]){
  assert.doesNotMatch(file,/crate::money|pub mod money/)
 }
 assert.doesNotMatch(app,/MoneyPage|pages\/Money/)
})
