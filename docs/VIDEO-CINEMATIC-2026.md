# 2026 hosted cinematic video
API/UI `/api/video/jobs` -> `video::enqueue` (frozen render spec) -> `jobs::run_job` -> `video::ltx::run` -> LTX V2 submit/receipt/poll/download -> private MP4 player -> distinct YouTube upload.
Pro/Fast, 720p–4K, 9:16/16:9/1:1 and generated audio are available after an encrypted API key is configured. Paid requests are not automatically resubmitted following uncertain POST responses; remote job IDs and in-flight markers persist, and restarts require review. MP4 downloads use bounded streaming and restricted HTTPS origins. Existing ComfyUI and native video are preserved.
Sources (2026): https://docs.ltx.io/models/ltx-2-5/ ; https://docs.ltx.io/async-jobs ; https://docs.ltx.io/api-changelog/ .
No tests, builds or paid API calls were run; real credential validation and full production media QA remain to be completed.
