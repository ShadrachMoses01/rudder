# Failure Matrix

Testing Rudder against real-world conditions to find reliability gaps.

## Case 1: Node project

**Setup:** `test-projects/node-app/` — simple Node.js with `npm run dev`

**Expected:** Detects `Dev` service with `npm run dev`

**Actual:** ✅ Detected `Dev (npm run dev)` — `package.json` has a `dev` script

**Notes:**

---

## Case 2: Rust project

**Setup:** `test-projects/rust-app/` — Cargo.toml, `cargo run`

**Expected:** Detects `Dev` service with `cargo run`

**Actual:** ✅ Detected `Dev (cargo run)` — `Cargo.toml` detected

**Notes:**

---

## Case 3: Node + Rust (monorepo root)

**Setup:** Run Rudder from a directory that has both `package.json` and `Cargo.toml`

**Expected:** Primary detection picks one. Sub-directories may catch the other.

**Actual:** ❓ Not tested — no test project has both at root level.

**Notes:** `detect_primary()` returns on first match (if-else chain). Only one primary service is created. This is a known limitation — if a project has both Node and Rust at root, only one gets detected.

---

## Case 4: Docker Compose

**Setup:** `test-projects/docker-app/` — `docker-compose.yml` with `redis` and `postgres`

**Expected:** Detects `Database (docker compose up postgres)` and `Redis (docker compose up redis)`

**Actual:** ✅ Detected `Database` and `Redis`

**Notes:**

---

## Case 5: Monorepo

**Setup:** `test-projects/monorepo/` — `rudder.toml` config with Frontend + Backend

**Expected:** Config is source of truth → only `Frontend` and `Backend` shown

**Actual:** ✅ Config-only mode works

**Notes:** Also verify that `rudder init --stdout` shows all detected sub-services.

---

## Case 6: Broken command

**Setup:** `test-projects/broken-project/` — `package.json` with `dev: "node server.js"` but `server.js` exists

**Expected:** Starts, exposes a URL. Actually this is not "broken" from Rudder's perspective.

**Actual:** ✅ Server starts, `http://localhost:{port}` detected from log output

**Notes:** Also tested `dev` script that runs a nonexistent command (`nonexistent-command-will-fail`). `npm run dev` spawned successfully (npm exists), but npm failed running the script. `refresh()` caught the non-zero exit code → `Status::Failed`. `rudder up` loop detected `any_alive == false` and exited cleanly.

---

## Case 7: Missing executable

**Setup:** Point Rudder at a project whose detected executable is not in PATH

**Expected:** `Status::Failed("Executable not found: <name> (not in PATH)")`

**Actual:** ✅ When a config explicitly sets `cmd = "some-nonexistent-binary"`, `Service::start()` calls `which::which(exe)` → returns `Err` → status set to `Failed("Executable not found: some-nonexistent-binary (not in PATH)")`.

**Notes:** For auto-detected services (e.g., `npm run dev`), the command is the package manager (`npm`/`pnpm`/etc.), which is virtually always in PATH. The missing executable case mostly applies to config-defined services.

---

## Case 8: Port already in use

**Setup:** Start a service that binds to a port, then start another on the same port

**Expected:** Second service logs `EADDRINUSE`, Rudder shows warning

**Actual:** ⚠️ Not yet tested — `refresh()` has logic to detect `EADDRINUSE` in logs

**Notes:** The `port_warn` logic in `Service::refresh()` should flag this. Need to verify it works end-to-end.

---

## Case 9: Crash during startup

**Setup:** Service process exits immediately with non-zero code

**Expected:** `Status::Starting` → `try_wait()` returns `Some(ExitStatus(...))` → `Status::Failed`

**Actual:** ✅ Tested with `npm run dev` where dev script runs a nonexistent command. `npm` exits with non-zero code → `refresh()` catches it → `Status::Failed(format!("exit code {}", code))`. `rudder up` exits because `any_alive == false`.

**Notes:** The transition works. Edge case: if the crash happens before the first `refresh()` call within `rudder up`'s loop, the service stays as `Starting` for up to 500ms.

---

## Case 10: Ctrl+C during startup

**Setup:** Start a service, press Ctrl+C before it finishes starting

**Expected:** Rudder stops the service, cleans up PID, no crash

**Actual:** ⚠️ Not yet tested manually. Signal handler sets `running = false` → loop exits → `s.stop()` called. `stop()` checks `self.child` exists → `kill_process_group()` → clean up.

**Notes:** Should work because the signal handler and stop path are the same regardless of state. Risk: if the process hasn't spawned yet (between `Command::spawn()` and `self.child = Some(child)`), the signal handler could fire during a window where `child` is still `None`. The current `stop()` would be a no-op (no child to kill), but the process could still be spawning.

---

## Case 11: PID file / process mismatch

**Setup:**
1. `rudder up` — starts services, writes PIDs
2. Service crashes externally (e.g., `kill -9` the process)
3. `rudder down` — reads stale PID file

**Expected:** `kill_process_group()` detects process is gone → "already stopped (stale PID file)" → PID file cleaned up

**Actual:** ✅ Tested with fake PID `999999`. `kill_process_group(999999)` → `process_exists()` returns false → returns `false`. `cmd_down` prints "already stopped (stale PID file)" and cleans up.

**Notes:** PID reuse risk remains: if the OS reuses PID 999999 for a new process before `rudder down` runs, `process_exists()` returns true, and we kill the wrong process. Mitigation: `kill -0` before `kill -15` narrows the window but doesn't eliminate it. A more robust solution would store process start time alongside PID.

---

## Case 12: `rudder up` then `rudder down` from separate terminal

**Setup:**
1. Terminal 1: `rudder up`
2. Terminal 2: `rudder down`

**Expected:** Terminal 2 reads PID files, kills process groups

**Actual:** ✅ Tested by running `rudder up &` in background, then `rudder down`. Terminal 2 read the PID file, killed the process group, `rudder up` detected all services stopped and exited cleanly.

**Notes:** Also verified `rudder up` exits when all services stop (not just on Ctrl+C). This was a bug found during testing — `rudder up` previously only exited on Ctrl+C, now also exits when all child processes die.

---

## Case 13: Directory with no project files

**Setup:** Run Rudder in `/tmp` or similar empty directory

**Expected:** "No services detected. Run `rudder init` to generate a config."

**Actual:** ✅ Works correctly — empty services list with helpful message

**Notes:**

---

## Case 14: Very large log output

**Setup:** Service that produces continuous log output (e.g., `while true; do echo "line"; done`)

**Expected:** Log buffer trims at 500 lines, no unbounded memory growth

**Actual:** ⚠️ `Service::refresh()` drains excess lines — need to verify under heavy output

**Notes:** The log reader threads are unbounded — they push to `Vec<String>` without backpressure. `refresh()` only trims after the fact.

---

## Case 15: Multiple rapid start/stop cycles

**Setup:** Rapidly press Enter on a running service (toggles stop → start)

**Expected:** No race conditions, no double-spawned processes

**Actual:** ⚠️ Not yet tested

**Notes:** `start()` checks `self.status != Running && self.status != Starting` — should prevent double spawn. `stop()` takes child and kills it. Need to verify no race between the two.

---

## Summary

| # | Case | Status | Priority |
|---|------|--------|----------|
| 1 | Node project | ✅ | - |
| 2 | Rust project | ✅ | - |
| 3 | Node + Rust root | ❓ Untested | Low |
| 4 | Docker Compose | ✅ | - |
| 5 | Monorepo with config | ✅ | - |
| 6 | Broken command | ✅ | Medium |
| 7 | Missing executable | ✅ | Medium |
| 8 | Port already in use | ⚠️ Untested | Medium |
| 9 | Crash during startup | ✅ | High |
| 10 | Ctrl+C during startup | ⚠️ Untested | High |
| 11 | Stale PID files | ✅ | High |
| 12 | Cross-terminal up/down | ✅ | High |
| 13 | Empty directory | ✅ | - |
| 14 | Heavy log output | ⚠️ Untested | Medium |
| 15 | Rapid start/stop | ⚠️ Untested | Medium |
