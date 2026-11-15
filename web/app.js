(() => {
  'use strict';
  const $ = id => document.getElementById(id);
  const state = { selected: null, runs: [], token: '', run: null };
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
      const [runs,config] = await Promise.all([request('/runs'),request('/config')]);
      state.runs = runs;
      $('config-list').textContent = [
        '기본 모델: '+config.model,'경제형 모델: '+config.economy_model,
        '인증/로컬 연결: '+(config.configured?'설정됨':'API 키 미설정'),
        '셸 도구: '+(config.shell_enabled?'승인 후 실행':'비활성'),
        '최대 기본 라운드: '+config.max_steps
      ].join('\n');
      status('● 서버 연결됨',true);
      renderList();
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
    ['runs','settings'].forEach(name=>$('page-'+name).classList.toggle('hidden',name!==button.dataset.page));
  }));
  $('connect').addEventListener('click',()=>{state.token=$('token').value;$('token').value='';load()});
  load();setInterval(()=>{if(!document.hidden)load()},2500);
})();
