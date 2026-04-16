# Netherica v0.2

Netherica is a Rust desktop ingestion/reporting application (egui/eframe) for Excel-based inventory flow. In its basic form a tool like Grist could probably manage this just fine. This exists purely as me dipping my toe into the 'vibe coding' everybody talks about. 

Most of it written in opencode. Main coder is Gemma 4 26B A4B UD-IQ3_S limit context to 70k token to prevent memory overflow on my 7800XT using llama.cpp as inference engine (I could not figure out how to make sgLang or vLLM work with my machine yet) 

When Gemma 4 can't handle the task, the task is handed over to GPT-5.4 (xhigh) for planing or GLM-5.1 when hit Codex limit, they also handle debugging. Coder mainly Gemma 4 26B and later GPT-5.3 Codex or GLM-5 or GLM-4.7 depends on avaialbility.

**The process of creation follows**

```
┌─────────────┐   ┌───────────┐   ┌─────────────┐   ┌────────────────┐
│   Written   │──→│   LLMs    │──→│  Opencode   │──→│    Opencode    │
│ Requirements│   │ Rewritten │   │ coding loop │   │ Refinement loop│
└─────────────┘   └───────────┘   └─────────────┘   └────────────────┘
```

- I start drafting written requirements by hand first. The requirements are fueled by my rage and deep hatred toward the inability of people to do their job properly. Legacy system output report with number as TEXT, but product ID as an INTEGER losing leading 0 in the process. People who pull reports are not fixing them before passing them to clerk. Clerk who only started a month ago, still under training needs handheld throughout the process of turning it to report, then left when become competent enough to be autonomy.

- That hatred fuels paragraph is then pass to LLMs to draft software requirements. The requirement was drafted with LLMs chat interface (not sophisticated agent conference) each was instructed to act as a group of senior engineers’ critic each other’s works, leveraging free tier as much as possible with human(me) as orchestrator. Main contributors obviously Sonet 4.6, Gemini 3.1 Pro and Gemini Thinking mode, GPT-chat (not sure about model), and Deepseek V3.2 (which surprisingly very logical). Then the LLMs then sliced it into Phase and Task, Thanks to plan mode.

- Coding loops consist of 3 agents, Main orchestrator agent, Engineer agent, and QA agent.
	- Orchestrator read the `TASKS.md` then read `REQUIREMENTS.md` picked the appropriate context then passed it to Engineer subagent to code.
	- Engineer code the specific section, run some basic `cargo check` then fix the error until it's passed, then pass it to QA
	- QA run `cargo check`, `cargo clippy -- -D warnings`, `cargo fmt --check`, and `cargo test`. If fix needed it's passed back to Engineer, all tests that are in requirement also apply at this step if all conditions are satisfactory, it returns [APPROVE] message back to the orchestrator which then will move to next task.
	
- This is the main loop until all phase is complete, yes, it’s basically structure YOLO mode, I just want to try delegating to subagents that's all.

- Next, to ensure the models did not slack off, which they often do. Main agent run `REQUIREMENTS.md` against codebase one more time, flag every missing requirement with [MISSING] along with source filename and line number, then created new phase in `TASKS.md` to fix all [MISSING] task and start the 3 agents loop again.

- Finally, human (me) inspection, if the workflow behave as intended, is the UI look alright, did the report formatted as I want, whatever I find unsatisfactory will be patched into `REQUIREMENTS.md` and `TASKS.md` then let the 3 agents fix it again.

**UI Rework**
- Again everything draft then refine by LLMs, even all prompt and subagent files also drafted by LLM. The design also generate by Stitch then basically follow the same process of Orchestrator-->Engineer-->QA loop.
- The hardest part is when agent want to check if the UI behave properly a lot of screenshot were taken at somepoint I start to think that maybe it'd be faster if I do the UI part myself, but I push through with AI, so some elements especially on parsing screen may still be rough around the edge, some element might jump around, but does not effect the workflow.
- Well yeah, I doubt anyone will find use for the exept me anyway.

## Runtime behavior

- Loads `config.toml` (auto-generates a default config on first run).
- Uses SQLite (`state.db`) with WAL mode.
- Writes archived source files to `archive/` and generated HTML reports to `reports/`. 
- In case local folder can't be create. The default fall back in Windows is `%APPDATA%\netherica\archive` and  `%APPDATA%\netherica\reports`. on Linux `~/.local/share/Netherica/archive` and `~/.local/share/Netherica/reports`
- Embeds report assets using `rust-embed` from `asset/`:
  - `templates/report.html.tera`
  - `fonts/Sarabun/Sarabun-Regular.ttf`
  - `fonts/NotoSansThaiLooped/NotoSansThaiLoop.ttf`
  - `font/Inter/Inter.ttf'`

Because assets are embedded at compile time, distribution binaries do **not** require an external `asset/` folder at runtime.

## How to Builds

Clone the repo the run
```
cargo build --release
```
Only executable is needed to be copy.
