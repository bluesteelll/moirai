# V5: recording the 16-agent load fixture

- **Status:** the owner's procedure for V5, written with WP-51 (R-HARN-I). Not run yet: the owner deferred every
  measurement on 2026-09-29 (`docs/m0/PLAN.md` §5), and V5 waits until he resumes them.
- **Sources:** `docs/m0/PLAN.md` §3.2 WP-51 and §5 (V5); `docs/spec/measurement-protocol.md` §2.3 and §9 (cited
  [MP §x]); [60 §5.1] (the Load row) and [60 §5.2] item 16; [AR §11] #35 (a resource profile, no content) and #37
  (it stays on the laptop).

## 1. What it is for

"Loaded" in every M0 measurement means the 16-agent load fixture: the system-wide processor, disk and memory counters
of one normal 16-agent campaign, replayed by `moirai-probes-bin loadgen` on the laptop with 1.8 GB of physical memory
left available ([MP §2.3]). V5 records that fixture once. Measurement 16 then checks that the replay reproduces it,
and every "loaded" row of WP-52 to WP-57 runs under it.

The campaign is a normal one: the usual 16 agents on the usual kind of work, for as long as it usually takes. Nothing
about the work needs to change, and nothing about it is recorded beyond the machine's totals. Commits and merges keep
working in every worktree while the recorder runs: the private manifest leaves out the file being recorded and the
stop request, so the pre-commit guard does not see them change ([MP §9.3]).

## 2. Before the campaign

1. Work in the main worktree: the fixture goes to its `/private/load/`, which git ignores and the pre-commit hook
   refuses (`docs/m0/PLAN.md` §2.5, §3.2 WP-03).
2. No measurement, benchmark or nightly window runs during the campaign; the campaign itself is the load.
3. Set the power plan so that the laptop does not sleep or hibernate while it is plugged in, for the whole campaign
   (Settings > System > Power: screen off is fine, sleep "Never"). A sleep pauses typeperf, and the fixture then holds
   the last sample before it as one long step, which the replay would reproduce as a stretch of constant load that
   the campaign never had.
4. Open one extra terminal window in the main worktree for the recorder and leave it open for the whole campaign.

## 3. Recording

1. In the extra window, start the recorder:

   ```
   cargo xtask loadrec start
   ```

   It prints the file it records into, waits for typeperf's first sample (up to a minute on a busy machine) and then
   prints `recording`. From then on it prints a line every minute.
2. Start the campaign as usual.
3. **Check after a few minutes.** Look at the recorder's window once its first two or three minute lines are out.
   Each says `missing values of the required counters: none` when the processor and disk counters all hold values;
   a few missing values are harmless. If a line starting `loadrec: WARNING:` names a counter that has had no value at
   all, the fixture would be of no use: press Enter to stop, and report the warning to the measurement session.
4. When the campaign has finished, stop the recorder: press Enter in its window, or run `cargo xtask loadrec stop` in
   any terminal in the repository.
5. The recorder prints the fixture's name (`/private/load/<start>.load`), its samples, its missing values (in all and
   per counter) and its BLAKE3, then rebuilds `/private/MANIFEST.b3` (a new file under `/private/` would otherwise
   make the private guard refuse the next commit).
6. Optionally, look at what was stored:

   ```
   cargo xtask loadrec check private/load/<start>.load
   ```

   It prints the 18 counter names and, for each, how many values it holds and their minimum, mean, p95 and maximum.

- **If the window was closed, the recorder was interrupted (Ctrl+C) or the laptop restarted** during the campaign,
  run `cargo xtask loadrec stop`: it seals what was recorded up to then (sealing never loses a recorded sample, even
  when it is itself interrupted). A new recording cannot start while an unsealed one is left.
- **The recorder stops by itself after 12 hours.** For a longer campaign, start it with `--max-duration <seconds>`.
- If the recorder refuses to start because typeperf cannot read one of the processor or disk counters, nothing is
  written; report the message to the measurement session.

## 4. What is recorded

Once a second, typeperf (built in to Windows) reads 18 performance counters, all of them totals of the whole machine
([MP §9.1]):

| Object | Counters |
|---|---|
| Processor, `_Total` | % Processor Time, % Privileged Time, % User Time |
| PhysicalDisk, `_Total` | Disk Read Bytes/sec, Disk Write Bytes/sec, Disk Reads/sec, Disk Writes/sec, Avg. Disk Queue Length |
| Memory | Available Bytes, Committed Bytes, Pages/sec |
| Paging File, `_Total` | % Usage |
| Process, `_Total` (all processes summed) | Private Bytes, Working Set, IO Read Bytes/sec, IO Write Bytes/sec, Thread Count, Handle Count |

Each sample is stored as the milliseconds since the first sample and 18 numbers. The file holds a fixed header, the 18
counter names and these numbers, and nothing else ([MP §9.2]). A typical campaign of a few hours gives a file of a few
megabytes.

## 5. What is not recorded

- **No content.** No prompt, transcript, file name or file content, window title, process name or command, network
  address or keystroke. The counters are numbers about the whole machine.
- **No single process.** Only the `_Total` instances: the Process counters are the sum over every process, so no
  process, agent or program can be told apart.
- **No identity and no clock.** typeperf prints the machine's name before every counter and the date and time of every
  sample; the recorder reads these in memory and drops them. The fixture keeps only the time since the recording began.
  A test proves that the stored file does not change whatever machine name, time zone and timestamps typeperf prints.
- **No trace.** WPR is not used, so no event trace (ETL) is written; typeperf itself writes no file.
- **It never leaves the laptop.** `/private/` is never committed, the pre-commit hook and the CI checks refuse it, and
  the hosted runners' noise runs (`noise.yml`) use only synthetic load, never the fixture.

## 6. Afterwards

- The fixture is named in every loaded run record by its BLAKE3 ([MP §2.3]); the measurement aggregates in
  `docs/measurements/m0/` hold summary numbers of runs, never the fixture or its samples.
- Measurement 16 replays the fixture with `moirai-probes-bin loadgen` in an agent-free window (V4) and checks the
  replay against it ([MP §9.5]); the noise bands of the laptop are measured in the same windows.
- To record a fixture again (another campaign, a changed machine), repeat §3; each recording is a new file, and the
  measurement session names the one it uses.
