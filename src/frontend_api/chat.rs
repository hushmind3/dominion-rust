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
pub(super) fn yes()->bool{true}
pub(super) async fn chat(State(engine):State<Arc<Engine>>,Json(input):Json<ChatInput>)
    ->Result<Sse<impl Stream<Item=Result<Event,Infallible>>>,Fault>{
    if input.input.trim().is_empty()||input.input.len()>10000{
        return Err(bad("chat input must be 1-10000 bytes"));
    }
    if input.reasoning_language.as_deref().is_some_and(|v|!matches!(v,"ko"|"original"|"default")){
        return Err(bad("unsupported response language preference"));
    }
    let model_only=!input.tools_enabled||!input.tools_execution_enabled;
    // A model-only turn makes exactly one no-tools request. The autonomous
    // harness is not launched, so tool disabled cannot silently execute tools.
    // Per-run learning is enforced by Run.strategy_learning in the Rust core.
    if input.reasoning_effort.as_deref().is_some_and(|x|!x.is_empty()){
        return Err(unsupported("Per-run reasoning effort is not yet supported by the configured Rust model adapter."));
    }
    if input.thinking==Some(false){
        return Err(unsupported("Per-run thinking disable is not yet supported by the configured Rust model adapter."));
    }
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
    let prior=record.messages.iter().rev().take(12).collect::<Vec<_>>();
    let history=prior.into_iter().rev().map(|msg|format!("{}: {}",msg.role,msg.content)).collect::<Vec<_>>().join("\n");
    let trimmed=if history.chars().count()>6500{history.chars().rev().take(6500).collect::<String>().chars().rev().collect::<String>()}else{history};
    let response_language=if input.reasoning_language.as_deref()==Some("ko"){
        "Write any user-visible explanation and final response in natural Korean."
    }else{"Use the user's requested response language."};
    let prompt=format!("USER-SUPPLIED AGENT PROFILE (follow within system/user policy):\n{}\n\nPRIOR CHAT RECORD (user-supplied; lower-trust):\n{}\n\nCURRENT USER REQUEST:\n{}\n\nRESPONSE LANGUAGE PREFERENCE:\n{}",
        agent.system_prompt,trimmed,input.input.trim(),response_language);
    if prompt.len()>20000{return Err(bad("chat context exceeds native agent 20k input limit"));}
    let mut run=Run::new(CreateRun{instruction:prompt,budget_krw:None,max_steps:None},
        engine.settings.max_steps).map_err(bad)?;
    run.strategy_learning=input.auto_learning;
    run.model_only=model_only;
    run.provider_id=selected_provider;
    run.model_name=selected_model;
    if run.provider_id!="dominion-native"{
        run.event("pricing_unknown","Provider has no verified KRW tariff: zero recorded cost is not zero real spending",None);
    }
    engine.store.put(run.clone()).await.map_err(broken)?;
    engine.frontend.begin_turn(&record.id,input.input.trim(),run.id).await.map_err(broken)?;
    if model_only{
        let worker=Arc::clone(&engine);
        let id=run.id;
        let messages=vec![
            json!({"role":"system","content":"Respond to the user's chat request accurately and directly. Do not claim to have executed tools or accessed files. No tools are available during this turn."}),
            json!({"role":"user","content":run.instruction.clone()}),
        ];
        worker.store.mutate(id,|item|{
            item.status=RunStatus::Running;
            item.event("model_only","Tool-free chat turn requested by the user",None);
            Ok(())
        }).await.map_err(broken)?;
        tokio::spawn(async move {
            // Drop the HTTP future on cancellation rather than continuing
            // an expensive provider call in the background.
            let selected_model=match worker.store.get(id).await{
                Some(run)=>match worker.model_for_run(&run){
                    Ok(model)=>model,
                    Err(error)=>{
                        let _=worker.store.mutate(id,|item|{
                            item.status=RunStatus::Failed;
                            item.event("provider_missing",error,None);Ok(())
                        }).await;
                        return;
                    }
                },
                None=>return,
            };
            let current=worker.store.get(id).await;
            if let Some(current)=current{
                worker.providers.throttle(&current.provider_id).await;
            }
            let answer=tokio::select!{
                result=selected_model.complete(&messages,&[],false)=>Some(result),
                _=async{
                    loop{
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        if worker.store.get(id).await.is_none_or(|item|
                            item.status!=RunStatus::Running){break;}
                    }
                }=>None,
            };
            let Some(answer)=answer else{return;};
            let saved=worker.store.mutate(id,|item|{
                // Cancellation can win while the provider is responding.
                // Never resurrect a cancelled run or publish its answer.
                if item.terminal() || item.status!=RunStatus::Running{return Ok(());}
                match answer {
                    Ok(ref reply)=>{
                        item.input_tokens=item.input_tokens.saturating_add(reply.input_tokens);
                        item.cached_input_tokens=item.cached_input_tokens.saturating_add(reply.cached_input_tokens);
                        item.cache_write_tokens=item.cache_write_tokens.saturating_add(reply.cache_write_tokens);
                        item.output_tokens=item.output_tokens.saturating_add(reply.output_tokens);
                        item.cost_krw+=reply.cost_krw;
                        item.event("model",
                            format!("{} · in {} / out {} tokens · {:.4} KRW",
                                reply.model,reply.input_tokens,reply.output_tokens,reply.cost_krw),
                            Some(json!({"tool_calls":0})));
                        if item.cost_krw>item.budget_krw{
                            item.status=RunStatus::Failed;
                            item.event("budget_exceeded","Model-only response exceeded the run budget",None);
                        }else if reply.text.trim().is_empty(){
                            item.status=RunStatus::Failed;
                            item.event("model_empty","Provider returned no content",None);
                        }else{
                            item.answer=Some(reply.text.clone());
                            item.status=RunStatus::Completed;
                            item.event("completed","Model-only chat turn completed without tools",None);
                        }
                    },
                    Err(ref error)=>{
                        item.status=RunStatus::Failed;
                        item.event("provider_error",error.clone(),None);
                    }
                }
                Ok(())
            }).await;
            if let Err(error)=saved{tracing::error!(%id,%error,"unable to persist model-only chat response");}
        });
    }else if let Err(e)=engine.launch(run.id).await{
        let _=engine.store.mutate(run.id,|item|{
            item.status=RunStatus::Failed;
            item.event("launch_error",e.clone(),None);
            Ok(())
        }).await;
        let _=engine.frontend.finish_turn(&record.id,run.id,None).await;
        return Err(bad(e));
    }
    let worker=Arc::clone(&engine);
    let session_id=record.id.clone();
    let run_id=run.id;
    tokio::spawn(async move{
        // Persist the answer even if a browser disconnects while the task runs.
        loop{
            match worker.store.get(run_id).await{
                Some(state) if state.terminal()=>{
                    if let Err(error)=sync_session(&worker,&session_id).await{
                        tracing::error!(%error,%session_id,"failed to persist finished chat");
                    }
                    break;
                },
                None=>break,
                _=>tokio::time::sleep(Duration::from_millis(350)).await,
            }
        }
    });
    let stream_engine=Arc::clone(&engine);
    let session_id=record.id;
    let id=run.id;
    let id_text=id.to_string();
    let stream=async_stream::stream!{
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
                    let error=json!({"type":"error","message":format!("native run ended with {:?}",run.status)});
                    yield Ok(Event::default().json_data(error).unwrap_or_else(|_|Event::default().data("{}")));
                }
                yield Ok(Event::default().json_data(json!({"type":"done"})).unwrap_or_else(|_|Event::default().data("{}")));
                break;
            }
            if run.status==RunStatus::Paused && run.in_flight.is_some(){
                let value=json!({"type":"error","message":"Tool outcome is uncertain; reconcile the native run before proceeding."});
                yield Ok(Event::default().json_data(value).unwrap_or_else(|_|Event::default().data("{}")));
                yield Ok(Event::default().json_data(json!({"type":"done"})).unwrap_or_else(|_|Event::default().data("{}")));
                break;
            }
            tokio::time::sleep(Duration::from_millis(350)).await;
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(8)).text("alive")))
}


