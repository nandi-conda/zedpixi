# Usability test: a first run with the pixi extension in Zed

A small, moderated test of the first-run experience: install the extension,
open a pixi project, get Zed's own tools running inside the pixi environment,
run a pixi task, and read what the extension says when something is wrong.

It is designed for one facilitator, five participants and one afternoon. The
goal is to find where first-time users stall, not to prove that the extension
works. Every task below is something the current code and docs claim to
support.

## What is being tested, and what is not

The extension is a dev extension that is not in Zed's registry. It resolves
`<workspace root>/pixi.toml` with `pixi info --json`, exposes the environment
and its `bin` directory, and starts the `pixi-gdb` and `pixi-lldb` debug
adapters from that environment. It registers **no language servers, context
servers or slash commands** (`extension.toml` has no such sections). Zed's
built-in language servers, tasks, terminals and formatters are routed to the
environment by project settings, written by `scripts/zed_pixi_sync.py`
(`pixi run zed-sync`).

That has two consequences for the test:

- **There is no `/pixi-env` slash command.** A useful probe is whether a
  participant looks for one anyway, but no task requires it. The nearest real
  checks are `pixi run zed-sync`, `pixi run zed-sync-check` and
  `pixi run env-check`.
- **`pixi-env-check` is a terminal tool, not an editor feature.** It is the
  native binary in `src/bin/pixi_env_check.rs`, run as `pixi run env-check`
  (or `cargo run --features native-check --bin pixi-env-check -- .`). It prints
  the resolved environment and the `PATH` project settings should carry, and
  exits nonzero when a requested tool is only on the host `PATH`.

A third consequence matters for the tasks: the extension has no command and no
activation hook that resolves the environment. `pixi::resolve` runs only when a
debug adapter is requested (`get_dap_binary`), so opening a project normally
produces no visible signal at all. The test therefore measures what participants
expect to see, where they look, and how they verify the environment without
such a signal. Errors from the extension's resolver may only appear in
`Zed.log`; do not assume a participant can see them.

Out of scope: Windows (the `Scripts` lookup is implemented but only the Linux
path is tested), extension development itself, and performance beyond "how long
did the participant wait".

## What participants will face

| Feature | What should happen | Failure mode to watch for |
| --- | --- | --- |
| Dev install | `zed: install dev extension` compiles `target/wasm32-wasip2/debug/zed_pixi.wasm` and shows no error | `Failed to install dev extension: compiling Rust extension`, with the reason only in `Zed.log`; `running rustc: No such file or directory` when Zed's `PATH` has no rustup |
| Workspace resolution | The workspace root must be the directory that contains `pixi.toml`; nothing is shown when it is | Opening a parent directory: `no pixi.toml at the workspace root (...)` when a debug adapter resolves, or no message at all in normal editing |
| Environment selection | `default` is used; a single environment is used | Two environments without `default`: `this project has no \`default\` pixi environment; available environments: test, docs` |
| Not-installed environment | `pixi info` reports a prefix | `pixi environment \`default\` has no prefix; run \`pixi install\`` |
| Missing pixi | Zed resolves the environment | `pixi was not found on PATH. Install pixi and make sure the \`pixi\` command is available to Zed.` |
| Built-in language servers | `.zed/settings.json` points `binary.path` at `<prefix>/bin/...` with `ignore_system_version` and an `env` block | Nothing visible changes in Zed; server still uses the host copy; participant cannot tell which binary is running |
| Tasks | `.zed/tasks.json` has `pixi: <task>` entries, run through `pixi run` | Task list empty because the generator was never run; task output is a terminal, easy to miss |
| Strict tool lookup | `pixi run env-check taplo python` shows `<prefix>/bin/...` and `shadows host ...` | A host-only tool prints `NOT IN ENVIRONMENT` and exits 1; participant reads this as a broken extension |
| Debug adapters | `pixi-lldb` / `pixi-gdb` start from the environment without generated settings | `\`lldb-dap\` is not in the pixi environment (checked <prefix>/bin/lldb-dap)`; or the participant picks Zed's `CodeLLDB` and gets the host binary |
| Shadow check | `pixi run env-check --shadow` prints `shadow check passed` | Needs a native Rust build; looks unrelated to Zed to a first-time user |

## Method at a glance

- **5 participants**, one 45-minute session each, moderated, think-aloud.
- **One facilitator.** Screen and audio recording recommended; a notes sheet
  with per-task timestamps is enough if recording is not allowed.
- **One machine**, one Zed version, one extension commit. Record both.
- **A 20-minute pilot** with a colleague who is not counted, to fix fixtures
  and wording. Do not change fixtures between real sessions.
- Run all sessions in one afternoon: five slots of 45 minutes with 15-minute
  gaps (about 4.5 hours), then 30 minutes to consolidate.

### Schedule

| Time | Activity |
| --- | --- |
| 0:00 | Welcome, consent, recording, think-aloud instruction (4 min) |
| 0:04 | Warm-up questions (2 min) |
| 0:06 | Tasks T1-T7 (30 min) |
| 0:36 | SUS and targeted questions (7 min) |
| 0:43 | Wrap-up, thank-you (2 min) |

Task budgets are short on purpose: the interesting measurements are where time
goes and when the first hint is needed. If a session overruns, drop T7 and run
it as a probe in the next session rather than shortening the questionnaire.
T8 is an optional extra 5 minutes when a session runs fast. T9 (debugging)
replaces T7 and makes the session 60 minutes; use it only with a participant
who already debugs in Zed.

## Participants and where to recruit

Five is enough for a first pass: after three or four sessions the same two or
three problems usually repeat.

Screening criteria:

- Uses a code editor daily; comfortable in a terminal.
- At least two participants already use pixi; at least two are new to pixi.
- At least two use Zed regularly; at least two are trying it.
- Nobody who wrote the extension or its docs.
- Linux or macOS. Prefer a mix if both are available.

Recruit from: the pixi Discord community channels, the Zed Discord, a local
Rust or Python meetup, and colleagues who use conda or pixi. Offer a 45-minute
slot and a small thank-you (coffee voucher or similar). Ask them to bring
nothing and not to read the README beforehand.

## Test environment and fixtures

Prepare one machine:

```sh
# Once, before the sessions
rustup target add wasm32-wasip2
pixi --version
zed --version
```

Use the extension repository checkout as the dev-extension source. Do not
install it before the session. If you want install time to reflect the
extension rather than the network, warm the cargo registry and build once in a
copy, then remove the installed dev extension; record that you did this.

Create three fixture projects under `~/usability/`:

```sh
# demo-py: the normal case. Pre-install the environment.
# This is a user's project, so it deliberately has no scripts/ directory and
# no zed-sync task: those live in the extension repository.
# pixi.toml:
#   [workspace] name = "demo-py" channels = ["conda-forge"]
#              platforms = ["linux-64", "osx-arm64"]
#   [dependencies]
#   python = ">=3.11"
#   rich = "*"
#   basedpyright = "*"
#   [tasks]
#   test = "python -c \"import rich; print('pixi test ok')\""
# demo.py:  from rich.console import Console
#           Console().print("hello")
#
# demo-two-envs: create with `pixi init`, add two environments (for example
# `test` and `docs`) and no `default`. Exact dependencies do not matter; the
# point is the environment-selection error.
#
# not-pixi: a README.md and a hello.py, no pixi.toml.
```

Run `pixi install` in `demo-py` and confirm `pixi run test` prints
`pixi test ok`. Confirm `command -v basedpyright` prints nothing on the host,
so a language server that reports diagnostics is definitely the environment
copy. Run `pixi run env-check` once in the extension repository before the first
session so the native build is warm; delete `target/` afterwards only if you
also accept the first participant paying that build time.

Optional probes:

- A `demo-py-uninstalled` copy with `.pixi/envs` removed, to see the
  `run \`pixi install\`` message.
- A `demo-py-with-scripts` copy that contains `scripts/zed_pixi_sync.py` and
  the `zed-sync` tasks, to compare "feature works" with "user can obtain the
  tool". Use it as a fourth task variant, not as a replacement.
- A stripped shell for T8: start Zed from a launcher whose `PATH` lacks
  `~/.pixi/bin`, or temporarily rename the `pixi` binary.

For T6, check beforehand that `git` is on the host but not in `demo-py`
(`command -v git` prints a path; `ls ~/usability/demo-py/.pixi/envs/default/bin/git`
fails). Use another host-only tool if `git` is in the environment.

The fixture deliberately mirrors a real user project: it has no
`scripts/zed_pixi_sync.py` and no `zed-sync` task, because the generator is
distributed in this repository. Running the generator against it therefore
means passing `--root` or copying the script. Whether a participant finds that
out is part of the test, and a likely friction point for the README, which
shows `pixi run zed-sync` as if every project defines it.

## Facilitation

### Think-aloud script (read aloud)

> "Thanks for helping. We're testing the extension and its documentation, not
> you. Nothing you do here can be wrong.
>
> While you work, please think out loud: say what you're looking at, what you
> expect to happen, and what surprises you. I'll mostly stay quiet so I don't
> influence you. If you're stuck for more than a couple of minutes, ask me and
> I'll help.
>
> I'll record the screen and your voice so I don't have to take notes while you
> work. Is that okay? You can stop the recording or the session at any time.
>
> One rule for the tasks: don't open the README unless you would normally do
> that when setting up a new tool. There are no trick questions; if something
> seems missing, that's useful for us."

### Prompts

Use neutral prompts only:

- "What are you thinking?"
- "What did you expect to happen there?"
- "What would you try next?"
- "How would you check that it worked?"
- "Where would you look for that?"

Do not say: "Did you see the ...", "Try the command palette", "It's in the
README", "You need to run zed-sync first".

### Intervention rules

| Situation | Action | Record as |
| --- | --- | --- |
| No progress for 2 minutes | One neutral prompt | still unaided |
| No progress for 3 minutes | Hint: name the area, not the step ("the docs describe a setup step") | assisted |
| No progress for 5 minutes | Show the step, then move on | assisted |
| Task impossible due to fixture or machine | Stop the task | environment failure, not participant failure |

Always note the time of the first hint. Time to first hint is often more
informative than completion time.

## Task protocol

Read the instruction, start the timer, and stop it at the success criterion.
Do not add steps the participant would not think of. If they ask "should I
...?", answer "what would you do?".

| # | Task | Success criterion | Budget |
| --- | --- | --- | --- |
| T1 | Install the extension | Dev install completes; no error toast; the extension is listed or the built wasm exists | 7 min |
| T2 | Open the pixi project | `demo-py` is the workspace root; participant can say whether it was recognized and how they checked | 3 min |
| T3 | Show which environment Zed should use | Environment name, prefix and `PATH` are visible for `demo-py` (from the generator, `pixi info --json`, or `pixi-env-check` run against it) | 4 min |
| T4 | Make the built-in Python language server use the environment | `demo-py/.zed/settings.json` has an absolute `<prefix>/bin/basedpyright-langserver` path; after reload, Zed runs that path, and `zed-sync --check` passes | 6 min |
| T5 | Run a pixi task from Zed | The `pixi: test` task runs and prints `pixi test ok` | 4 min |
| T6 | Prove host tools do not leak in | `pixi-env-check` against `demo-py` with `git` (host-only) reports `NOT IN ENVIRONMENT` and exits nonzero; `python` resolves under the prefix | 3 min |
| T7 | Explain a failure | Open `demo-two-envs` or a copy without `pixi.toml`; participant finds the relevant message and says what to do | 3 min |
| T8 | Optional missing-pixi probe | With `pixi` absent from Zed's `PATH`, participant reports the message and a recovery step | 5 min |
| T9 | Optional debugging | `pixi add lldb`, a `pixi-lldb` launch config, breakpoint hit in a Rust binary | 15 min |

### Task details and what to watch

**T1 - Install the extension.**
Instruction: "This repository is the extension. Make Zed use it."
Watch: whether they find `zed: install dev extension`; whether they select the
repository root or a subdirectory; whether they wait or retry during the cargo
build; the exact toast text. If the build fails, note whether they open
`zed: open log` or reach for the troubleshooting doc. Record the failing log
line, for example `running rustc: No such file or directory`, and whether
starting Zed from a terminal fixes it.

**T2 - Open the pixi project.**
Instruction: "Open the demo project and tell me whether Zed sees it as a pixi
project."
Watch: opening the parent folder versus `demo-py` itself; whether they look for
a status indicator, command or notification. There is no UI marker, so the
interesting result is what they expect to see and how long they search before
trying a terminal command.

**T3 - Show the environment.**
Instruction: "Show me which pixi environment and `PATH` Zed should be using for
this project."
Watch: the paths they try. `pixi run env-check` exists only in this repository
and checks the repository unless given a root argument, so a participant working
in `demo-py` has to use `pixi info --json`, the generator's output, or
`pixi-env-check` with the fixture path. Record which one they choose, how long
it takes, and whether they expect the editor to show this. Note whether the
printed prefix and `PATH` make sense to them, especially that the environment's
`bin` comes first.

**T4 - Route the language server.**
Instruction: "Get the Python language server to use the project's environment,
then show me it is using the environment's copy."
Watch: whether they find `scripts/zed_pixi_sync.py` in the extension repository
and pass `--root`, or copy it into the project; whether they run it before or
after opening the project; whether they edit `.zed/settings.json` by hand;
whether they reload Zed. Verify by the configured absolute path, by the running
process (`ps` shows `demo-py/.pixi/envs/default/bin/basedpyright-langserver`),
and by `zed-sync --check`. Import resolution is a secondary signal: if
`rich` imports cleanly but `ps` shows the host server, routing failed.
Common failure: the generator was run from the wrong directory, so
`.zed/settings.json` appears in the extension repo or in the home directory; or
the participant expects the extension to do this automatically.

**T5 - Run a pixi task.**
Instruction: "Run the project's test task from inside Zed."
Watch: whether they use the task picker (`task: spawn`, default `alt-t`) or the
terminal; whether they find `pixi: test`; whether they understand that the task
runs `pixi run test`; what the terminal panel shows. If the task list is empty,
the generator did not write `demo-py/.zed/tasks.json`; ask what they expected to
be there and record the elapsed time. Running `pixi run test` by hand is a
workaround, not success, for this task.

**T6 - Strict host lookup.**
Instruction: "Show that a tool installed only on your machine is not used by
this project."
Run it against the fixture, for example from the extension repository:

```sh
cargo run --features native-check --bin pixi-env-check -- \
  ~/usability/demo-py python git
```

Success is `python: <prefix>/bin/python` plus
`git: NOT IN ENVIRONMENT (...)` and a nonzero exit. Watch: whether nonzero exit
is read as a failure of the extension; whether they read `shadows host` and
`NOT IN ENVIRONMENT` correctly; whether they try `--shadow`. If the native
build was not warmed, exclude the build time and say so.

**T7 - Explain a failure.**
Instruction: "Open this second project and get its environment working."
The extension resolves lazily, so opening the project alone may show nothing.
Let the participant try what they would normally do. If they do not reach a
message within a minute, ask them to start a `pixi-lldb` debug session (the
adapter is the path that calls `pixi::resolve`) or to run the generator, which
prints `this project has no \`default\` pixi environment; available
environments: test, docs`. Success is that they can name the two environments
and the next step, not that the extension showed a toast. Watch: whether any
message appears at all, and where they looked: toast, status bar,
`zed: open log`, terminal. "The participant never saw an error" is a finding,
not a participant mistake.

**T8 - Missing pixi.**
Instruction: none; the facilitator sets up the stripped `PATH` beforehand.
Ask the participant to open `demo-py` and explain what happened. Record the
message, whether it names the fix, and whether they recover before time runs
out.

## What to record

For every task, fill one row:

| Field | Values |
| --- | --- |
| Task | T1-T9 |
| Outcome | unaided / assisted / failed / not run |
| Time | seconds to success, or to abandonment |
| First hint | seconds, or none |
| Error text | verbatim, and where it was visible (toast, log, terminal, none) |
| Confusion | one sentence in the participant's words |
| Docs used | file or section, or none |
| SEQ | 1 (very hard) to 7 (very easy), asked immediately after the task |

Also capture, once per session:

- Zed version, OS, pixi version, extension commit.
- Whether Zed was launched from a terminal or the desktop.
- Prior pixi and Zed experience (1-5 each).
- Any expectation stated before the first task ("I thought it would ...").

Definitions: **unaided** means no hint; **assisted** means a hint or a shown
step; **failed** means the success criterion was not reached. Do not count a
fixture or machine problem as a participant failure.

## Questionnaire

Ask the SUS first, then the targeted questions. Keep it short and let the
participant answer aloud if typing is slow.

### System Usability Scale (SUS)

Five-point scale: 1 = strongly disagree, 5 = strongly agree. Standard scoring:
odd items use `score - 1`, even items use `5 - score`; sum and multiply by 2.5
for a 0-100 score.

1. I think that I would like to use this extension frequently.
2. I found the extension unnecessarily complex.
3. I thought the extension was easy to use.
4. I think that I would need the support of a technical person to be able to
   use this extension.
5. I found the various functions in this extension were well integrated.
6. I thought there was too much inconsistency in this extension.
7. I would imagine that most people would learn to use this extension very
   quickly.
8. I found the extension very cumbersome to use.
9. I felt very confident using the extension.
10. I needed to learn a lot of things before I could get going with this
    extension.

### Targeted questions

Rate 1-5 (1 = strongly disagree, 5 = strongly agree):

1. Installing the extension was straightforward.
2. I could tell whether Zed was using the pixi environment.
3. Running a pixi task from Zed was straightforward.
4. When something went wrong, the message told me what to do next.
5. I would set this up again on another project without asking for help.

Short answers:

6. Before today, what did you expect the extension to do that it does not do?
7. Did you look for an editor command, button or slash command to show the pixi
   environment? What did you try, and what did you expect it to be called?
8. Which document did you open, and did it answer your question? If not, what
   was missing?
9. What was the most confusing moment, and what would have helped?
10. What one change would save you the most time? (optional) Would you
    recommend this to a teammate? (0-10)

## Analysis, targets and decisions

Spend 30 minutes after the last session, in this order:

1. Transcribe the per-task rows into one table.
2. Count unaided completions and medians of time to success and time to first
   hint per task.
3. Cluster the confusion notes into themes (install, "is it on?", verification,
   tasks, errors). Count how many participants hit each theme.
4. Rank by frequency x severity (severity: 1 = annoyance, 3 = blocked, 5 =
   gave up or formed a wrong belief).
5. Write the top three fixes with the evidence for each (task, participant,
   quote, time).

Targets, set before the first session:

| Signal | Target |
| --- | --- |
| Dev install unaided | 4 of 5 within 10 minutes, excluding first cargo build |
| Open the right workspace root | 5 of 5 unaided |
| Find a way to verify the environment | 3 of 5 unaided within 3 minutes |
| Language server clearly using the environment after docs | 4 of 5 unaided |
| Run `pixi: test` from Zed | 4 of 5 unaided |
| Median SUS | 68 or higher (75 or higher is a good result) |
| Participants blocked waiting for a slash command | 0 |

A missed target is a finding to report, not a reason to change the task
mid-study. If a fixture fails, note it and exclude that task for that session.

## Materials checklist

- [ ] Laptop with Zed, rustup with `wasm32-wasip2`, pixi, and the extension
      repository.
- [ ] Three fixture projects prepared and `demo-py` installed.
- [ ] `pixi run env-check` warmed once; decide and record whether `target/`
      stays warm.
- [ ] Screen recorder and microphone; consent line ready.
- [ ] Notes sheet with the task table, SUS, targeted questions, and a clock.
- [ ] Timer visible to the facilitator, not to the participant.
- [ ] Thank-you and a way to collect follow-up consent.
- [ ] Nothing installed in Zed before the first session.

## Limitations

Five participants give directional findings, not statistics. One facilitator
taking notes while moderating will miss quotes; recording is worth it. The
tasks cover the documented paths, not every platform: Windows and detached
environments are untested here. The test measures the first run, not daily use,
and it does not measure whether the generated settings survive dependency
changes.
