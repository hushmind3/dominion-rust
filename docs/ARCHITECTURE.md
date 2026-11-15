# Dominion Sub4 Architecture

## Single source of execution truth

```
Studio (web/app.js) ──┐
                      ├── HTTP /api/v1 ──> Rust Engine ──> ModelClient (provider.rs)
TypeScript SDK ───────┘         │                │
                                │                ├── file/search/write
                                │                ├── approved shell
                                │                └── provider tool results
                                │
               ┌────────────────┼────────────────┐
               ▼                ▼                ▼
           RunStore           Groups           Videos
          JSON checkpoint   shared history   scenes → FFmpeg → MP4
               │
          events / replay
               │
             Studio             Improvements: proposal → detached
                                  worktrees → frozen tests/score
                                    → evaluated → explicit adoption
```

All API handlers are in `src/api.rs`. They never call a separate UI-agent runtime. Config is `src/config.rs`; persisted domain types are `src/domain.rs`. The orchestrator `src/engine.rs` controls provider calls and tool results. `src/tools.rs` implements scoped artifacts and approved subprocesses. `src/provider.rs` owns OpenAI-compatible model transport, token usage, fallback and price estimation. Feature services are separate in `src/group.rs`, `src/video.rs` and `src/improvement.rs`. The executable bootstraps them in `src/main.rs`.

## Autonomous execution lifecycle

1. `POST /api/v1/runs` validates instruction, maximum model turns and KRW budget, persists the initial snapshot, then starts a background task.
2. The economy model proposes a concise plan; the main model selects available functions using native tool calls. File results stay in a per-run workspace.
3. Each model/tool result updates the server snapshot and monotone event sequence, visible through `GET /runs/{id}` or replayable SSE.
4. Requests to execute shell commands suspend the run in `awaiting_approval`; the pending tool arguments survive restarts. `approve` persists the decision and resumes exactly that run.
5. A candidate final answer is verified with a separate model call that sees the execution history and actual workspace file list. `REVISE` returns control to the execution loop.
6. Runs terminate as completed, failed or cancelled. Runs interrupted during process shutdown reopen as paused and are resumable.
7. Runtime limits: configurable 1..100 model rounds, 128 KB read cap, 256 KB write cap, 200 listed files, up to 40 s per approved shell command, local paths only. The shell environment removes inherited provider API keys.

The executable default port is `127.0.0.1:8787`. Remote bind requires bearer authentication; each API route is protected. Browser assets are served from the same origin.

## Collaboration and production

**Groups:** `Groups` stores participant identity, role, economy/main tier, shared utterances and max turns. User mentions pick a named participant; absent a mention, role overlap and speaker diversity guide selection. A model-produced `[END]` may end the room. This is sequential collaboration, not yet concurrent tool-capable subagents.

**Video:** `Videos::create` takes a supplied scene list or calls a model for JSON scenes. `Videos::start` renders 1280×720 scenes via FFmpeg with text overlays, optional espeak, AAC audio and H.264 video. Source scene clips are retained for partial replacement. The final artifact is `runtime/videos/<id>/finished.mp4`. Render attempts and job states persist; a server restart marks interrupted rendering failed so the user can rerun it. No paid video endpoint is required.

**Non-weight improvement:** `Improvements` restricts edits to a designated source file of a clean Git checkout under the configured workspace. The fixed test command and optional numeric score command are captured before proposal. On explicit approval, two detached worktrees share a frozen base revision, the model proposes a candidate, and the same tests/evaluator are run independently. Only an objective improvement becomes adoptable. `adopt` checks the original HEAD remains frozen and cherry-picks the candidate commit. `descendant` inherits the prior measured reflection. Inference-time reflection and strategy evolution do not alter model weights.

## Audit limitations and next integrations

- Durable JSON snapshot writes use temp-and-rename, not a transactional multi-writer database or fsync-based power-loss guarantee.
- Model API cost is estimated from configured unit prices and provider usage. Prompt caching and cost ceilings are not guaranteed when rates are unconfigured. Remote GPU/video compute costs are not metered.
- File paths reject parent traversal and symlinks, but race-free OS filesystem capabilities (e.g. openat2) and process-level sandboxing have not been integrated.
- MCP servers, external web/browser skills, video synthesis providers, cross-device auth and genuinely parallel tool-writing subagents are future connection points.
- Independent benchmark harnesses such as SWE-bench Verified are not included; functional tests establish different, narrower evidence.
