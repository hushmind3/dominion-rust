(() => {
  'use strict';
  const $ = id => document.getElementById(id);
  const state = { selected: null, runs: [], token: '', run: null, selectedGroup:null, groups:[], group:null, selectedVideo:null, videos:[], video:null, selectedImprovement:null, improvements:[], improvement:null };
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
      const [runs,config,groups,videos,improvements] = await Promise.all([request('/runs'),request('/config'),request('/groups'),request('/videos'),request('/improvements')]);
      state.groups=groups;state.videos=videos;state.improvements=improvements;
      state.runs = runs;
      $('config-list').textContent = [
        '기본 모델: '+config.model,'경제형 모델: '+config.economy_model,
        '인증/로컬 연결: '+(config.configured?'설정됨':'API 키 미설정'),
        '셸 도구: '+(config.shell_enabled?'승인 후 실행':'비활성'),
        '최대 기본 라운드: '+config.max_steps,
        '연결된 MCP 서버: '+(config.mcp_server_count??0)
      ].join('\n');
      status('● 서버 연결됨',true);
      renderGroupList();renderVideoList();renderImproveList();
      renderList();
      if (state.selectedGroup) { const group=groups.find(g=>g.id===state.selectedGroup);if(group && (!state.group || state.group.updated_at_ms!==group.updated_at_ms))renderGroup(group); }
      else if(groups.length){state.selectedGroup=groups[0].id;renderGroup(groups[0]);}
      if (state.selectedVideo) { const video=videos.find(v=>v.id===state.selectedVideo);if(video && (!state.video || state.video.updated_at_ms!==video.updated_at_ms))renderVideo(video); }
      else if(videos.length){state.selectedVideo=videos[0].id;renderVideo(videos[0]);}
      if(state.selectedImprovement){const i=improvements.find(x=>x.id===state.selectedImprovement);if(i&&(!state.improvement||state.improvement.updated_at_ms!==i.updated_at_ms))renderImprove(i);}
      else if(improvements.length){state.selectedImprovement=improvements[0].id;renderImprove(improvements[0]);}
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
    if(run.pending_approval && !run.in_flight) content+=`<div class="approval"><b>⚠️ 명령 실행 승인 필요</b><code>${safe(run.pending_approval.name + ': '+ (run.pending_approval.arguments.command || JSON.stringify(run.pending_approval.arguments)))}</code><div class="actions"><button class="primary" id="allow">승인</button><button class="danger" id="deny">거부</button></div></div>`;
    if(run.in_flight) content+=`<div class="approval"><b>⚠️ 중단된 도구 확인 필요</b><p>실제로 도구가 실행됐을 수 있습니다. 파일·출력을 확인해 결과를 기록하면 중복 실행 없이 작업을 재개할 수 있습니다.</p><code>${safe(run.in_flight.name)} · ${safe(JSON.stringify(run.in_flight.arguments))}</code><label for="reconcile-result">확인한 실행 결과 또는 오류</label><textarea id="reconcile-result" placeholder="직접 확인한 결과를 적어주세요."></textarea><div class="actions"><button class="primary" id="reconcile-ok">성공 결과 기록</button><button class="danger" id="reconcile-failed">실패 결과 기록</button></div></div>`;
    if(run.answer) content+=`<h2 style="margin-top:20px">완료 결과</h2><div class="output">${safe(run.answer)}</div>`;
    content+=`<div class="actions" style="margin-top:20px">`;
    if(run.status==='paused' && !run.in_flight) content+=`<button class="primary" id="resume">▶ 재개</button>`;
    if(!['completed','failed','cancelled'].includes(run.status)) content+=`<button class="danger" id="cancel">중단</button>`;
    content+=`</div>`;
    if(!['completed','failed','cancelled'].includes(run.status)) content+=`<div style="margin-top:16px"><label for="directive">추가 지시</label><input id="directive" placeholder="진행 중 요구사항을 수정할 수 있습니다."><button class="secondary" id="send-directive" style="margin-top:9px">지시 전달</button></div>`;
    $('run-detail').innerHTML=content;
    $('timeline').innerHTML=run.events.length?run.events.slice(-60).reverse().map(e=>`<div class="event"><time>${time(e.at_ms)} · ${safe(e.kind)}</time><strong>${safe(e.message)}</strong></div>`).join(''):'<p>이벤트 없음</p>';
    const command=async(path,body={})=>{try{await request('/runs/'+run.id+'/'+path,{method:'POST',body:JSON.stringify(body)});await load()}catch(err){alert(err.message)}};
    $('allow')?.addEventListener('click',()=>command('approval',{allow:true}));
    $('deny')?.addEventListener('click',()=>command('approval',{allow:false}));
    $('reconcile-ok')?.addEventListener('click',()=>command('reconcile',{succeeded:true,result:$('reconcile-result').value}));
    $('reconcile-failed')?.addEventListener('click',()=>command('reconcile',{succeeded:false,result:$('reconcile-result').value}));
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
    ['runs','groups','video','improve','settings'].forEach(name=>$('page-'+name).classList.toggle('hidden',name!==button.dataset.page));
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
      const body={prompt:$('video-prompt').value,backend:$('video-backend').value};
      if(manual)body.scenes=JSON.parse(manual);
      const v=await request('/videos',{method:'POST',body:JSON.stringify(body)});
      state.selectedVideo=v.id;state.video=null;$('video-error').textContent='렌더 시작';await load();
    }catch(error){$('video-error').textContent=error.message}
    finally{$('video-create').disabled=false}
  });

  function renderImproveList(){
    $('improve-list').innerHTML=state.improvements.length?state.improvements.map(i=>`<button class="run ${i.id===state.selectedImprovement?'selected':''}" data-improve="${safe(i.id)}"><div class="run-title">${safe(i.instruction)}</div><div class="run-meta"><span>${safe(i.status)}</span><span>${safe(i.target_file)}</span><span>${krw(i.cost_krw)}원</span></div></button>`).join(''):'<p>아직 실험이 없습니다.</p>';
    document.querySelectorAll('[data-improve]').forEach(button=>button.addEventListener('click',()=>{
      state.selectedImprovement=button.dataset.improve;renderImproveList();renderImprove(state.improvements.find(i=>i.id===state.selectedImprovement));
    }));
  }
  function renderImprove(i){
    if(!i)return;state.improvement=i;$('improve-status').textContent=i.status;
    const outcome=m=>m?`<span style="font-weight:600">${m.tests_passed?'테스트 통과':'테스트 실패'}</span><p>점수: ${m.score??'설정 없음'}</p><div class="output" style="margin-top:9px;max-height:170px">${safe(m.log)}</div>`:'<p>아직 평가하지 않았습니다.</p>';
    let content=`<p>${safe(i.instruction)}</p><div class="metrics">
      <div class="metric"><span>대상 파일</span><b style="font-size:12px;overflow-wrap:anywhere">${safe(i.target_file)}</b></div>
      <div class="metric"><span>모델 비용</span><b>${krw(i.cost_krw)}원</b></div>
      <div class="metric"><span>개선 판정</span><b>${i.accepted?'향상':'미충족'}</b></div></div>
      <h2>실행 대상 · 승인 범위</h2><div class="output">${safe(i.repository)} / ${safe(i.target_file)}\n\n테스트: ${safe(i.test_command)}\n평가 점수: ${safe(i.score_command||'없음')}</div>`;
    if(i.error)content+=`<div class="approval" style="margin-top:15px">${safe(i.error)}</div>`;
    if(i.base_commit)content+=`<p class="muted" style="margin:12px 0">기준 커밋: ${safe(i.base_commit)}</p>`;
    if(i.proposal)content+=`<p class="muted" style="margin:12px 0">제안 변경: ${safe(i.proposal)}</p>`;
    content+=`<div class="workspace" style="grid-template-columns:1fr 1fr;gap:9px;margin-top:20px"><div><h2>동결 기준 평가</h2>${outcome(i.baseline)}</div><div><h2>후보 평가</h2>${outcome(i.candidate)}</div></div>`;
    if(i.reflection)content+=`<h2 style="margin-top:18px">다음 세대에 전달할 개선 기록</h2><div class="output">${safe(i.reflection)}</div>`;
    content+='<div class="actions" style="margin-top:19px">';
    if(i.status==='awaiting_approval')content+='<button class="primary" id="improve-approve">평가 명령 승인 후 실행</button>';
    if(i.status==='evaluated'&&i.accepted)content+='<button class="primary" id="improve-adopt">검증된 변경 채택</button>';
    if(i.status==='adopted')content+='<button class="secondary" id="improve-descendant">학습 기록을 이어서 다음 후보 만들기</button>';
    content+='</div>';
    $('improve-detail').innerHTML=content;
    const post=async(path)=>{try{$('improve-error').textContent='처리 중';const result=await request('/improvements/'+i.id+'/'+path,{method:'POST',body:'{}'});$('improve-error').textContent='';if(path==='descendant'){state.selectedImprovement=result.id;state.improvement=null;}await load();}catch(error){$('improve-error').textContent=error.message}};
    $('improve-approve')?.addEventListener('click',()=>{if(confirm('격리된 Git 작업공간에서 지정한 셸 테스트/점수 명령을 실행하시겠습니까?'))post('approve')});
    $('improve-adopt')?.addEventListener('click',()=>{if(confirm('원본 Git 저장소에 검증된 후보 커밋을 적용하시겠습니까?'))post('adopt')});
    $('improve-descendant')?.addEventListener('click',()=>post('descendant'));
  }
  $('improve-create').addEventListener('click',async()=>{
    try{
      const input={repository:$('improve-repo').value,target_file:$('improve-file').value,
        instruction:$('improve-instruction').value,test_command:$('improve-test').value};
      const score=$('improve-score').value.trim();if(score)input.score_command=score;
      const improvement=await request('/improvements',{method:'POST',body:JSON.stringify(input)});
      state.selectedImprovement=improvement.id;state.improvement=null;$('improve-error').textContent='실험 대기 생성';await load();
    }catch(error){$('improve-error').textContent=error.message}
  });
  load();setInterval(()=>{if(!document.hidden)load()},2500);
})();
