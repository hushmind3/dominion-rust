(() => {
  'use strict';
  const $ = id => document.getElementById(id);
  const state = { selected: null, runs: [], token: '', run: null, selectedGroup:null, groups:[], group:null, selectedVideo:null, videos:[], video:null };
  const safe = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const krw = value => Number(value || 0).toLocaleString('ko-KR',{maximumFractionDigits:3});
  const time = ms => new Date(ms).toLocaleTimeString('ko-KR',{hour:'2-digit',minute:'2-digit',second:'2-digit'});
  const short = id => String(id).slice(0,8);
  async function request(route, options = {}) {
    const headers = { ...(options.body ? {'Content-Type':'application/json'} : {}), ...(state.token ? {'Authorization':'Bearer '+state.token} : {}) };
    const response = await fetch('/api/v1'+route,{ ...options, headers });
    let value;
    try { value = await response.json(); } catch { value = {}; }
    if (!response.ok) throw new Error(value.error || 'HTTP '+response.status);
    return value;
  }
  function status(text, okay) {
    $('connection').textContent = text;
    $('connection').style.borderColor = okay ? '#397d70' : '#9d6b60';
  }
  async function load() {
    try {
      const [runs,config,groups,videos] = await Promise.all([request('/runs'),request('/config'),request('/groups'),request('/videos')]);
      state.groups=groups;state.videos=videos;
      state.runs = runs;
      $('config-list').textContent = [
        '기본 모델: '+config.model,'경제형 모델: '+config.economy_model,
        '인증/로컬 연결: '+(config.configured?'설정됨':'API 키 미설정'),
        '셸 도구: '+(config.shell_enabled?'승인 후 실행':'비활성'),
        '최대 기본 라운드: '+config.max_steps
      ].join('\n');
      status('● 서버 연결됨',true);
      renderGroupList();renderVideoList();
      renderList();
      if (state.selectedGroup) { const group=groups.find(g=>g.id===state.selectedGroup);if(group && (!state.group || state.group.updated_at_ms!==group.updated_at_ms))renderGroup(group); }
      else if(groups.length){state.selectedGroup=groups[0].id;renderGroup(groups[0]);}
      if (state.selectedVideo) { const video=videos.find(v=>v.id===state.selectedVideo);if(video && (!state.video || state.video.updated_at_ms!==video.updated_at_ms))renderVideo(video); }
      else if(videos.length){state.selectedVideo=videos[0].id;renderVideo(videos[0]);}
      if (state.selected) {
        const found = runs.find(r=>r.id===state.selected);
        if (found) renderRun(found); else state.selected=null;
      } else if (runs.length) {
        state.selected = runs[0].id;renderRun(runs[0]);
      }
    } catch(error) { status('연결 오류',false); $('create-message').textContent = error.message; }
  }
  function renderList() {
    $('run-list').innerHTML = state.runs.length ? state.runs.map(run=>`<button class="run ${run.id===state.selected?'selected':''}" data-id="${safe(run.id)}"><div class="run-title">${safe(run.instruction)}</div><div class="run-meta"><span>${safe(run.status)}</span><span>${short(run.id)}</span><span>${krw(run.cost_krw)}원</span></div></button>`).join(''):'<p>작업이 없습니다.</p>';
    document.querySelectorAll('.run').forEach(button=>button.addEventListener('click',()=>{
      state.selected=button.dataset.id;renderList();renderRun(state.runs.find(r=>r.id===state.selected));
    }));
  }
  function renderRun(run) {
    if (!run) return;
    state.run=run;
    $('run-status').textContent=run.status;
    let content=`<p>${safe(run.instruction)}</p><div class="metrics">
      <div class="metric"><span>API 예상 비용</span><b>${krw(run.cost_krw)}원</b></div>
      <div class="metric"><span>실행 라운드</span><b>${run.steps}/${run.max_steps}</b></div>
      <div class="metric"><span>모델 토큰</span><b>${(run.input_tokens+run.output_tokens).toLocaleString()}</b></div></div>`;
    if(run.plan) content+=`<h2>실행 계획</h2><div class="output">${safe(run.plan)}</div>`;
    if(run.pending_approval) content+=`<div class="approval"><b>⚠️ 명령 실행 승인 필요</b><code>${safe(run.pending_approval.arguments.command||'')}</code><div class="actions"><button class="primary" id="allow">승인</button><button class="danger" id="deny">거부</button></div></div>`;
    if(run.answer) content+=`<h2 style="margin-top:20px">완료 결과</h2><div class="output">${safe(run.answer)}</div>`;
    content+=`<div class="actions" style="margin-top:20px">`;
    if(run.status==='paused') content+=`<button class="primary" id="resume">▶ 재개</button>`;
    if(!['completed','failed','cancelled'].includes(run.status)) content+=`<button class="danger" id="cancel">중단</button>`;
    content+=`</div>`;
    if(!['completed','failed','cancelled'].includes(run.status)) content+=`<div style="margin-top:16px"><label for="directive">추가 지시</label><input id="directive" placeholder="진행 중 요구사항을 수정할 수 있습니다."><button class="secondary" id="send-directive" style="margin-top:9px">지시 전달</button></div>`;
    $('run-detail').innerHTML=content;
    $('timeline').innerHTML=run.events.length?run.events.slice(-60).reverse().map(e=>`<div class="event"><time>${time(e.at_ms)} · ${safe(e.kind)}</time><strong>${safe(e.message)}</strong></div>`).join(''):'<p>이벤트 없음</p>';
    const command=async(path,body={})=>{try{await request('/runs/'+run.id+'/'+path,{method:'POST',body:JSON.stringify(body)});await load()}catch(err){alert(err.message)}};
    $('allow')?.addEventListener('click',()=>command('approval',{allow:true}));
    $('deny')?.addEventListener('click',()=>command('approval',{allow:false}));
    $('resume')?.addEventListener('click',()=>command('resume'));
    $('cancel')?.addEventListener('click',()=>command('cancel'));
    $('send-directive')?.addEventListener('click',()=>command('directive',{instruction:$('directive').value}));
    renderFiles(run.id);
  }
  async function renderFiles(id) {
    try {
      const {files}=await request('/runs/'+id+'/files');
      if(state.selected!==id)return;
      $('files').innerHTML=files.length?files.map(file=>`<button class="filelink" data-path="${safe(file)}">📄 ${safe(file)}</button>`).join(''):'<p>아직 파일이 없습니다.</p>';
      document.querySelectorAll('.filelink').forEach(button=>button.addEventListener('click',async()=>{
        try{const data=await request('/runs/'+id+'/files/'+button.dataset.path.split('/').map(encodeURIComponent).join('/')); const blob=new Blob([data.content],{type:'text/plain;charset=utf-8'});const a=document.createElement('a');a.href=URL.createObjectURL(blob);a.download=button.dataset.path.split('/').pop();a.click();setTimeout(()=>URL.revokeObjectURL(a.href),2000)}catch(err){alert(err.message)}
      }));
    } catch {if(state.selected===id)$('files').innerHTML='<p>파일 목록을 읽을 수 없습니다.</p>'}
  }
  $('create').addEventListener('click',async()=>{
    $('create').disabled=true;$('create-message').textContent='작업 생성 중';
    try {
      const run=await request('/runs',{method:'POST',body:JSON.stringify({instruction:$('instruction').value,budget_krw:Number($('budget').value),max_steps:Number($('steps').value)})});
      state.selected=run.id;$('instruction').value='';$('create-message').textContent='실행을 시작했습니다.';await load();
    }catch(err){$('create-message').textContent=err.message}finally{$('create').disabled=false}
  });
  document.querySelectorAll('nav button').forEach(button=>button.addEventListener('click',()=>{
    document.querySelectorAll('nav button').forEach(b=>b.classList.toggle('active',b===button));
    ['runs','groups','video','settings'].forEach(name=>$('page-'+name).classList.toggle('hidden',name!==button.dataset.page));
  }));
  $('connect').addEventListener('click',()=>{state.token=$('token').value;$('token').value='';load()});

  function renderGroupList(){
    $('group-list').innerHTML = state.groups.length ? state.groups.map(g=>`<button class="run ${g.id===state.selectedGroup?'selected':''}" data-group="${safe(g.id)}"><div class="run-title">${safe(g.topic)}</div><div class="run-meta"><span>${g.rounds}/${g.max_rounds} 발언</span><span>${krw(g.cost_krw)}원</span></div></button>`).join(''):'<p>그룹이 없습니다.</p>';
    document.querySelectorAll('[data-group]').forEach(button=>button.addEventListener('click',()=>{
      state.selectedGroup=button.dataset.group;renderGroupList();renderGroup(state.groups.find(g=>g.id===state.selectedGroup));
    }));
  }
  function renderGroup(g){
    if(!g)return;
    state.group=g;$('group-status').textContent=g.status;
    const roles = g.participants.map(p=>`<span class="pill">${safe(p.name)} · ${p.economy?'경제형':'주력'}</span>`).join(' ');
    let text=`<p>${safe(g.topic)}</p><div class="actions" style="margin:15px 0">${roles}</div>
       <div class="metrics"><div class="metric"><span>발언 라운드</span><b>${g.rounds}/${g.max_rounds}</b></div><div class="metric"><span>예상 모델 비용</span><b>${krw(g.cost_krw)}원</b></div><div class="metric"><span>남은 예산</span><b>${krw(Math.max(0,g.budget_krw-g.cost_krw))}원</b></div></div>
       <h2>공유 대화</h2><div class="timeline" style="max-height:520px">`;
    text+=g.messages.map(m=>`<div class="event"><time>${safe(m.speaker)} · ${time(m.at_ms)}${m.model?' · '+safe(m.model):''}</time><div style="white-space:pre-wrap;line-height:1.6;margin-top:6px">${safe(m.content)}</div></div>`).join('') || '<p>참가자에게 첫 발언을 요청해 보세요.</p>';
    text+='</div>';
    if(g.status==='open'){
      text+=`<div style="margin-top:20px"><label for="group-say">사용자 메시지 · @참가자명으로 발언 지목</label><textarea id="group-say" style="min-height:75px" placeholder="@설계자 지금 제약조건을 설명해 주세요."></textarea></div><div class="actions" style="margin-top:10px"><button id="group-send" class="secondary">메시지 전달</button><button id="group-next" class="primary">다음 참가자 발언</button></div>`;
    }
    $('group-detail').innerHTML=text;
    const post=async(path,body)=>{try{$('group-error').textContent='처리 중';await request('/groups/'+g.id+'/'+path,{method:'POST',body:JSON.stringify(body)});$('group-error').textContent='';await load();}catch(error){$('group-error').textContent=error.message}};
    $('group-send')?.addEventListener('click',()=>post('messages',{content:$('group-say').value}));
    $('group-next')?.addEventListener('click',()=>post('next',{}));
  }
  function renderVideoList(){
    $('video-list').innerHTML=state.videos.length?state.videos.map(v=>`<button class="run ${v.id===state.selectedVideo?'selected':''}" data-video="${safe(v.id)}"><div class="run-title">${safe(v.prompt)}</div><div class="run-meta"><span>${safe(v.status)}</span><span>${v.scenes.length}장면</span><span>${krw(v.cost_krw)}원</span></div></button>`).join(''):'<p>영상 작업이 없습니다.</p>';
    document.querySelectorAll('[data-video]').forEach(button=>button.addEventListener('click',()=>{
      state.selectedVideo=button.dataset.video;renderVideoList();renderVideo(state.videos.find(v=>v.id===state.selectedVideo));
    }));
  }
  function renderVideo(v){
    if(!v)return;
    state.video=v;$('video-status').textContent=v.status;
    let content=`<p>${safe(v.prompt)}</p><div class="metrics"><div class="metric"><span>장면</span><b>${v.scenes.length}</b></div>
    <div class="metric"><span>영상 길이</span><b>${v.scenes.reduce((acc,s)=>acc+s.seconds,0)}초</b></div>
    <div class="metric"><span>예상 모델 비용</span><b>${krw(v.cost_krw)}원</b></div></div>`;
    if(v.error) content+=`<div class="approval"><b>렌더 오류</b><code>${safe(v.error)}</code></div>`;
    content+='<h2>스토리보드</h2>';
    content+=v.scenes.map((scene,i)=>`<div class="panel" style="padding:15px;margin:10px 0"><div class="detail-head"><b>${i+1}. ${safe(scene.heading)}</b><span class="tag">${scene.seconds}초</span></div>
      <p style="white-space:pre-wrap">${safe(scene.narration)}</p><button class="secondary" style="margin-top:12px" data-scene="${i}" ${v.status==='rendering'?'disabled':''}>이 장면 수정</button></div>`).join('');
    content+='<div class="actions" style="margin-top:16px">';
    if(v.status==='completed')content+='<button class="primary" id="video-download">MP4 다운로드</button>';
    if(['draft','failed','completed'].includes(v.status))content+='<button class="secondary" id="video-render">현재 장면으로 렌더</button>';
    if(v.status==='rendering')content+='<span class="muted">FFmpeg 렌더 중 · 자동 갱신됩니다.</span>';
    content+='</div>';
    $('video-detail').innerHTML=content;
    const post=async(path,body)=>{try{$('video-error').textContent='처리 중';await request('/videos/'+v.id+path,{method:'POST',body:JSON.stringify(body)});$('video-error').textContent='';await load();}catch(error){$('video-error').textContent=error.message}};
    $('video-render')?.addEventListener('click',()=>post('/render',{}));
    document.querySelectorAll('[data-scene]').forEach(button=>button.addEventListener('click',()=>{
      const i=Number(button.dataset.scene),scene=v.scenes[i];
      const heading=prompt('장면 제목',scene.heading);if(heading===null)return;
      const narration=prompt('내레이션',scene.narration);if(narration===null)return;
      const seconds=Number(prompt('장면 길이 (2~30초)',String(scene.seconds)));
      post('/scenes/'+i,{heading,narration,seconds});
    }));
    $('video-download')?.addEventListener('click',async()=>{
      try {
        const headers=state.token?{Authorization:'Bearer '+state.token}:{};
        const response=await fetch('/api/v1/videos/'+v.id+'/download',{headers});
        if(!response.ok)throw new Error('영상 다운로드 오류: HTTP '+response.status);
        const file=await response.blob();const url=URL.createObjectURL(file);
        const a=document.createElement('a');a.href=url;a.download='dominion-'+short(v.id)+'.mp4';a.click();
        setTimeout(()=>URL.revokeObjectURL(url),3000);
      }catch(error){$('video-error').textContent=error.message}
    });
  }
  $('group-create').addEventListener('click',async()=>{
    try{
      const participants=$('group-members').value.trim().split('\n').filter(Boolean).map(line=>{
        const [name,role,model='main']=line.split('|').map(x=>x.trim());
        return {name,role,economy:model.toLowerCase()==='economy'};
      });
      const g=await request('/groups',{method:'POST',body:JSON.stringify({
        topic:$('group-topic').value,participants,budget_krw:Number($('group-budget').value),max_rounds:Number($('group-rounds').value)
      })});
      state.selectedGroup=g.id;state.group=null;$('group-error').textContent='그룹 생성 완료';await load();
    }catch(error){$('group-error').textContent=error.message}
  });
  $('video-create').addEventListener('click',async()=>{
    $('video-create').disabled=true;$('video-error').textContent='스토리보드 생성 · 준비 중';
    try{
      const manual=$('video-scenes').value.trim();
      const body={prompt:$('video-prompt').value};
      if(manual)body.scenes=JSON.parse(manual);
      const v=await request('/videos',{method:'POST',body:JSON.stringify(body)});
      state.selectedVideo=v.id;state.video=null;$('video-error').textContent='렌더 시작';await load();
    }catch(error){$('video-error').textContent=error.message}
    finally{$('video-create').disabled=false}
  });
  load();setInterval(()=>{if(!document.hidden)load()},2500);
})();
