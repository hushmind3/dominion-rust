import {test,expect} from 'playwright/test'

const roomId='33333333-3333-4333-8333-333333333333'
const catalog=[
 {id:'telegram',name:'Telegram',category:'messaging',mode:'outbound',description:'회의실 메시지 송신',
  available:true,docs:'https://core.telegram.org/bots/api',features:['메시지 보내기'],
  requiresRoom:true,fields:[{key:'token',label:'Bot Token',type:'secret',required:true},
   {key:'chatId',label:'Chat ID',type:'text',required:true}]},
 {id:'discord',name:'Discord',category:'community',mode:'outbound',description:'회의실 메시지 송신',
  available:true,docs:'https://discord.com/developers/docs/resources/message',
  features:['메시지 보내기'],requiresRoom:true,
  fields:[{key:'token',label:'Bot Token',type:'secret',required:true},
   {key:'channelId',label:'Channel ID',type:'text',required:true}]}
]
type Binding={
 id:string;name:string;kind:string;roomId:string;enabled:boolean;syncRoomMessages:boolean;
 chatId?:string;channelId?:string;status:string;configured:boolean;
 credentialsPresent:{token:boolean};delivery:{pending:number;needsReview:number;sending:number;pendingIndex:number|null}
}
function mockConnections(page:any,initial:Binding[]=[]){
 let bindings=[...initial]
 const requests:any[]=[]
 const reconcile:string[]=[]
 const ready=async()=>{
  await page.route('**/api/channels/catalog',(route:any)=>route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(catalog)}))
  await page.route('**/api/rooms',(route:any)=>route.fulfill({status:200,contentType:'application/json',body:JSON.stringify([{id:roomId,name:'운영 회의실',agentIds:['a']}])}))
  await page.route('**/api/channels',(route:any)=>{
   if(route.request().method()==='GET')return route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(bindings)})
   const body=route.request().postDataJSON()
   requests.push(body)
   const record:Binding={
    id:body.id||'connection-1',name:body.name,kind:body.kind,roomId:body.roomId,
    chatId:body.chatId,channelId:body.channelId,enabled:body.enabled,
    syncRoomMessages:body.syncRoomMessages,status:'not_verified',configured:true,
    credentialsPresent:{token:true},delivery:{pending:0,needsReview:0,sending:0,pendingIndex:null}
   }
   bindings=bindings.filter(x=>x.id!==record.id).concat(record)
   return route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(record)})
  })
  await page.route('**/api/channels/*/diagnose',(route:any)=>{
   bindings=bindings.map(x=>({...x,status:'outbound_ready'}))
   return route.fulfill({status:200,contentType:'application/json',body:JSON.stringify({verified:true,message:'Bot and destination verified by live API'})})
  })
  await page.route('**/api/channels/*/delivery/reconcile',(route:any)=>{
   const {resolution}=route.request().postDataJSON()
   reconcile.push(resolution)
   bindings=bindings.map(x=>({...x,status:'outbound_ready',delivery:{pending:0,needsReview:0,sending:0,pendingIndex:null}}))
   return route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(bindings[0])})
  })
 }
 return {ready,requests,reconcile}
}

test('outbound channel setup exposes only supported inputs and verifies after save',async({page})=>{
 const fixture=mockConnections(page)
 await fixture.ready()
 await page.goto('/#/connections')
 const tabs=page.locator('.connection-tabs').getByRole('button')
 await expect(tabs).toHaveCount(6)
 const telegram=page.getByRole('heading',{name:'Telegram'}).locator('xpath=ancestor::section[1]')
 await telegram.getByRole('button',{name:'연결 설정'}).click()
 const dialog=page.getByRole('dialog',{name:'새 연결 설정'})
 await expect(dialog).toBeVisible()
 await expect(dialog.getByText('Webhook 경로')).toHaveCount(0)
 await expect(dialog.getByText('허용한 외부 봇')).toHaveCount(0)
 await expect(dialog.getByText('발언자 선택 정책')).toHaveCount(0)
 await dialog.getByLabel('연결 이름').fill('운영 알림')
 await dialog.getByLabel('연결할 회의실').selectOption(roomId)
 await dialog.getByLabel(/Bot Token/).fill('123456:fixture-token')
 await dialog.getByLabel(/Chat ID/).fill('-100123456')
 await dialog.getByRole('button',{name:'설정 저장'}).click()
 await expect(page.getByText('운영 알림',{exact:true})).toBeVisible()
 await expect(telegram.getByText('송신 설정됨')).toBeVisible()
 expect(fixture.requests).toHaveLength(1)
 expect(fixture.requests[0].roomId).toBe(roomId)
 expect(fixture.requests[0].chatId).toBe('-100123456')
 expect(fixture.requests[0]).not.toHaveProperty('agentId')
 expect(fixture.requests[0]).not.toHaveProperty('webhookPath')
 expect(fixture.requests[0]).not.toHaveProperty('replyMentionBots')
})

test('uncertain outbound delivery is paused until explicit observed resolution',async({page})=>{
 const fixture=mockConnections(page,[{
  id:'pending-1',name:'Discord 운영',kind:'discord',roomId,channelId:'123456789',
  enabled:true,syncRoomMessages:true,status:'needs_review',configured:true,
  credentialsPresent:{token:true},
  delivery:{pending:1,needsReview:1,sending:0,pendingIndex:2}
 }])
 await fixture.ready()
 await page.goto('/#/connections')
 await expect(page.getByText('전송 결과 확인 필요').first()).toBeVisible()
 await page.getByRole('button',{name:'전송 결과 처리'}).click()
 const dialog=page.getByRole('dialog',{name:'전송 결과 확정'})
 await expect(dialog).toBeVisible()
 await dialog.getByRole('button',{name:/전송됨 확인/}).click()
 await expect(dialog).toHaveCount(0)
 expect(fixture.reconcile).toEqual(['confirmed_delivered'])
 await expect(page.getByText('전송 완료로 기록하고 다음 메시지로 이동했습니다.')).toBeVisible()
})
