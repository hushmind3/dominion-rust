//! Sub4 main-UI chat adapter; endpoint behavior preserved.
use super::*;

#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub(super) struct ChatInput {
    session_id:String,agent_id:String,input:String,
    provider_id:Option<String>,model:Option<String>,
    #[serde(default="yes")]tools_enabled:bool,
    #[serde(default="yes")]tools_execution_enabled:bool,
    #[serde(default)]auto_learning:bool,
    reasoning_effort:Option<String>,
    reasoning_language:Option<String>,
    thinking:Option<bool>
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub(super) struct InferenceSupportQuery{
    provider_id:String,
    model:String
}
pub(super) async fn inference_support(
    State(engine):State<Arc<Engine>>,Query(input):Query<InferenceSupportQuery>
)->Result<Json<Value>,Fault>{
    let model=engine.model_for_selection(&input.provider_id,&input.model)
        .map_err(bad)?;
    Ok(Json(model.run_reasoning_support()))
}

pub(super) fn yes()->bool{true}

/// 2026 cache-aware chat projection: role-preserving, non-generative, and
/// append-stable on the first four immutable conversation records. The
/// remaining recent records form a bounded suffix; messages never receive
/// a different role or authority when copied into a native Run.
pub(super) async fn chat(State(engine):State<Arc<Engine>>,Json(input):Json<ChatInput>)
    ->Result<Sse<impl Stream<Item=Result<Event,Infallible>>>,Fault>{
    if input.input.trim().is_empty()||input.input.len()>10000{
        return Err(bad("chat input must be 1-10000 bytes"));
    }
    if input.reasoning_language.as_deref().is_some_and(|v|!matches!(v,"ko"|"original"|"default")){
        return Err(bad("unsupported response language preference"));
    }
    let model_only=!input.tools_enabled||!input.tools_execution_enabled
        ||super::chat_context::no_tools_required(input.input.trim());
    // A model-only turn makes exactly one no-tools request. The autonomous
    // harness is not launched, so tool disabled cannot silently execute tools.
    // Per-run learning is enforced by Run.strategy_learning in the Rust core.
    sync_session(&engine,&input.session_id).await.map_err(broken)?;
    let record=engine.frontend.session(&input.session_id).await.ok_or_else(||missing("session not found"))?;
    if record.room_id.is_some(){return Err(bad("post room messages through the room API; direct chat is disabled for a room-linked session"));}
    if record.pending_run.is_some(){return Err(busy("another turn in this session is running"));}
    let agent=engine.frontend.agents().await.into_iter()
        .find(|a|a.id==input.agent_id).ok_or_else(||bad("agent not found"))?;
    if input.agent_id!=record.agent_id{return Err(bad("session belongs to another agent; create or fork a session"));}
    let (selected_provider,selected_model)=chosen(&engine,
        input.provider_id.as_deref().or(Some(record.provider_id.as_str())),
        input.model.as_deref().or(Some(record.model.as_str())))?;
    // Admission and transport share the same exact endpoint+model matrix.
    // An unsupported override is rejected before creating a durable Run or
    // charging a provider; supported fields are frozen with the Run receipt.
    let requested_effort=input.reasoning_effort.as_deref()
        .filter(|level|!level.is_empty());
    let requested_thinking=input.thinking.filter(|enabled|!*enabled);
    engine.model_for_selection(&selected_provider,&selected_model)
        .map_err(bad)?
        .with_run_reasoning(requested_effort,requested_thinking)
        .map_err(|error|unsupported(&error))?;
    // Old chat packed profile + transcript + current request into one new
    // opaque user string on EVERY turn. The changing string invalidated
    // provider prefix caches and made earlier assistant replies look like
    // owner instructions. Preserve actual roles and stable early evidence.
    let compacted=engine.frontend.compacted(&record.id).await;
    let context_budget=engine.settings.context_max_chars
        .saturating_sub(input.input.chars().count())
        .saturating_sub(if model_only{1_800}else{12_000});
    let profile=super::chat_context::compile(
        &agent.system_prompt,compacted.as_ref(),&record.messages,input.input.trim(),
        input.reasoning_language.as_deref()==Some("ko"),context_budget)?;
    let run=Dominion::from_engine(Arc::clone(&engine))
        .prepare_chat_run(CreateRun{instruction:input.input.trim().into(),
            budget_krw:None,max_steps:None},
            crate::sdk::RunOptions{
                provider_id:selected_provider,model:selected_model,
                strategy_learning:input.auto_learning,model_only
            }).await.map_err(bad)?;
    // Each UI message is a new isolated native Run, but all messages in the
    // same UI chat must retain ONE provider cache/routing session. Hash the
    // stable frontend session ID before it ever leaves the local runtime.
    use sha2::Digest;
    let identity=sha2::Sha256::digest(record.id.as_bytes());
    let suffix=identity[..16].iter().map(|v|format!("{v:02x}")).collect::<String>();
    engine.store.mutate(run.id,|current|{
        current.provider_session_affinity=format!("dominion-chat-{suffix}");
        current.reasoning_effort=requested_effort.map(str::to_owned);
        current.thinking=requested_thinking;
        current.chat_context=profile.messages;
        current.event("chat_context_attached","Role-preserving, bounded chat context attached",
            Some(json!({"messages":current.chat_context.len(),
                "current_request_chars":current.instruction.chars().count(),
                "projection_chars":profile.total_chars,
                "stable_prefix_chars":profile.stable_chars,
                "recent_chars":profile.recent_chars,
                "recalled_messages":profile.recalled_messages,
                "omitted_history_messages":profile.omitted_messages,
                "cache_hit_claimed":false})));
        Ok(())
    }).await.map_err(broken)?;
    if let Err(error)=engine.frontend.begin_turn(&record.id,input.input.trim(),run.id).await{
        // The competing/invalid turn never acquired the session lease.
        // Its prepared Run must not remain queued for a later worker wake.
        let reason=error.clone();
        engine.store.mutate(run.id,|current|{
            current.status=RunStatus::Failed;
            current.event("external_owner_admission_rejected",reason,None);
            Ok(())
        }).await.map_err(broken)?;
        return Err(if error.contains("active run"){busy(error)}else{bad(error)});
    }
    // This second durable write is the commit point of the owner/Run saga.
    // A crash before it leaves an unlaunchable fenced Run. The frontend
    // session is never allowed to start a model until this binding exists.
    if let Err(error)=engine.store.mutate(run.id,|current|{
        current.event("external_owner_admission_bound",
            "Durable chat session accepted this exact native Run",
            Some(json!({"owner_kind":"chat","session_id":record.id})));
        Ok(())
    }).await{
        let _=engine.frontend.finish_turn(&record.id,run.id,None).await;
        return Err(broken(error));
    }
    // Both tool agents and tool-free chat now launch through the same native
    // worker/cancellation lifecycle. No independent model-only executor.
    if let Err(error)=engine.launch(run.id).await{
        let _=engine.store.mutate(run.id,|item|{
            item.status=RunStatus::Failed;
            item.event("launch_error",error.clone(),None);
            Ok(())
        }).await;
        let _=engine.frontend.finish_turn(&record.id,run.id,None).await;
        return Err(bad(error));
    }
    let worker=Arc::clone(&engine);
    let session_id=record.id.clone();
    let run_id=run.id;
    tokio::spawn(async move{
        // A single event-driven finalizer persists the result even if the
        // browser disconnects. No per-chat periodic Run history scans.
        let mut changes=worker.store.subscribe();
        loop{
            match worker.store.get(run_id).await{
                Some(state) if state.terminal()=>{
                    if let Err(error)=sync_session(&worker,&session_id).await{
                        tracing::error!(%error,%session_id,"failed to persist finished chat");
                    }
                    break;
                },
                None=>break,
                _=>if !worker.store.changed(run_id,&mut changes).await{break;},
            }
        }
    });
    let stream_engine=Arc::clone(&engine);
    let session_id=record.id;
    let id=run.id;
    let id_text=id.to_string();
    let stream=async_stream::stream!{
        // Subscription is established before the first snapshot, closing the
        // start/read/listen lost-update gap.
        let mut updates=stream_engine.store.subscribe();
        let mut sequence=0u64;
        let mut last_approval:Option<String>=None;
        loop{
            let Some(run)=stream_engine.store.get(id).await else{break;};
            for event in &run.events{
                if event.seq<=sequence{continue;}
                sequence=event.seq;
                let output=match event.kind.as_str(){
                    "tool_started"=>Some(json!({"type":"tool_start","name":event.message})),
                    "tool_result"=>Some(json!({"type":"tool_end","name":event.message})),
                    "tool_error"=>Some(json!({"type":"tool_error","error":event.message})),
                    _=>None,
                };
                if let Some(value)=output{
                    yield Ok::<_,Infallible>(Event::default().json_data(value).unwrap_or_else(|_|Event::default().data("{}")));
                }
            }
            if run.status==RunStatus::AwaitingApproval{
                if let Some(call)=&run.pending_approval{
                    if last_approval.as_deref()!=Some(&call.id){
                        last_approval=Some(call.id.clone());
                        let value=json!({"type":"approval","id":id.to_string(),"tool":call.name,"args":call.arguments});
                        yield Ok(Event::default().json_data(value).unwrap_or_else(|_|Event::default().data("{}")));
                    }
                }
            }
            if run.terminal(){
                // Worker and stream can race; only actual persisted assistant message counts.
                let _=sync_session(&stream_engine,&session_id).await;
                let saved=stream_engine.frontend.session(&session_id).await;
                if let Some(message)=saved.and_then(|s|s.messages.into_iter().rev()
                    .find(|m|m.role=="assistant"&&m.metadata.as_ref().and_then(|v|v.get("executionId"))
                        .and_then(Value::as_str)==Some(id_text.as_str()))){
                    let out=json!({"type":"text","text":message.content});
                    yield Ok(Event::default().json_data(out).unwrap_or_else(|_|Event::default().data("{}")));
                    let saved=json!({"type":"assistant_saved","messageId":message.id});
                    yield Ok(Event::default().json_data(saved).unwrap_or_else(|_|Event::default().data("{}")));
                }else{
                    let error=json!({"type":"error","message":format!("native run ended with {:?}",run.status),
                        "runId":id_text,"status":run.status});
                    yield Ok(Event::default().json_data(error).unwrap_or_else(|_|Event::default().data("{}")));
                }
                yield Ok(Event::default().json_data(json!({"type":"done"})).unwrap_or_else(|_|Event::default().data("{}")));
                break;
            }
            if run.status==RunStatus::Paused{
                // A paused Run is not terminal, but a browser SSE response
                // must never hang forever waiting for a user-only recovery
                // decision. End this stream; the original Run stays durable.
                let reason=if run.in_flight.is_some(){
                    "Tool outcome is uncertain; reconcile the native Run before proceeding."
                }else if crate::journal::unsettled_model_request(&run).is_some(){
                    "Agent model request may already have been billed. Original Run is frozen; never repeat its plan, action turn or audit before independent review."
                }else if run.model_only&&run.events.iter().any(|e|
                    e.kind=="model_only_request_intent"){
                    "Model-only provider outcome is uncertain and may have been billed. Open the original Run; inference will not be replayed."
                }else{
                    "Execution paused. Open the original Run to inspect the error and resume only when safe."
                };
                let value=json!({"type":"error","message":reason,
                    "runId":id_text,"status":"paused"});
                yield Ok(Event::default().json_data(value).unwrap_or_else(|_|Event::default().data("{}")));
                yield Ok(Event::default().json_data(json!({"type":"done"})).unwrap_or_else(|_|Event::default().data("{}")));
                break;
            }
            if !stream_engine.store.changed(id,&mut updates).await{break;}
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(8)).text("alive")))
}


