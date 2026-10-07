export const meta = {
  name: 'unit',
  description: 'One moirai unit on its role branch: cut -> critique -> implement -> (mechanic gate -> tester) || review -> triage -> fix -> delta re-verify; local commits only',
  whenToUse: 'Launched from the unit session in the role worktree, as docs/m0/workflow.md section 6 describes',
  phases: [ { title: 'Cut' }, { title: 'Critique' }, { title: 'Implement' }, { title: 'Verify' }, { title: 'Fix' } ],
}
// The unit workflow of docs/m0/workflow.md section 6. Arguments (args, or embedded by <ORCH>/kit/embed.ps1, which
// replaces the line below in the per-launch copy <ORCH>/embed/<unit>.js):
//   role (author role, xtask/roles.toml), unit, orch, wt (the role worktree), gateWt (the lane's gate worktree),
//   branch (m0/<role>), base (the master sha the unit started from), trunk (master), spec (a path), lock, others,
//   gates (the per-commit gate commands), diskFloorGb, resumeNote, dryRun.
// A resumeNote reaches only the implement stage, so a relaunch replays the cut and the critique from the journal.
// A resumeNote that starts "DELTA SINCE <sha>:" makes round 1 delta-scoped since that verified head.
const A = args || {} // EMBED: kit/embed.ps1 replaces this line in the per-launch copy

const NEED = ['role', 'unit', 'orch', 'wt', 'gateWt', 'branch', 'base', 'trunk', 'spec', 'lock', 'others', 'gates', 'diskFloorGb']
const missing = NEED.filter(k => A[k] === undefined || A[k] === null || A[k] === '')
if (missing.length) return { ok: false, stage: 'args', missing }
if (A.branch !== 'm0/' + A.role) return { ok: false, stage: 'args', error: 'branch must be m0/<role>' }
if (A.dryRun) return { ok: true, args: A }

const SP = A.orch + '/' + A.unit + '/'
const DM = /^DELTA SINCE ([0-9a-f]{7,40})\b/.exec(A.resumeNote || '')
const SINCE0 = DM ? DM[1] : ''
const HDR = `UNIT ${A.unit}, author role ${A.role}. Spec: ${A.spec}. Rulings: ${A.orch}/RULINGS.md. Procedure: docs/m0/workflow.md.
- Author role ${A.role} binds you: PLAN §3.1 (S1-S6), its deny_read in xtask/roles.toml and the write map of docs/m0/authors.md §3. deny_read binds every way of reading, Bash included (cat, rg, git show <rev>:<path>, git diff over those paths); a file you need but may not read is a review finding in your report, never a read.
- Every shell command starts with cd into your directory: ${A.wt} unless your stage names another. Write only in ${A.wt}, under ${SP} and in the scratch worktrees your stage makes; delete only by absolute path under ${SP}. Under ${A.orch} read only ${SP}, kit/ and RULINGS.md: the baseline and other units' directories hold other roles' files and test names.
- Branch ${A.branch} from ${A.base}; trunk ${A.trunk}. Lock set: ${A.lock}. Other open units: ${A.others}. A file outside the lock set, or in another unit's set, is a STOP.
- Per-commit gates: ${A.gates}. The full verdict is cargo xtask gate --branch ${A.branch} in ${A.gateWt}, run by the mechanic through ${A.orch}/kit. A pre-flight below ${A.diskFloorGb} GB of free disk is a STOP.
- Long runs go through ${A.orch}/kit (start-gate.ps1, wait-gate.ps1, stop-gate.ps1) under a budget; wait in bounded slices; past the budget is a hang: stop it by its own PID tree, never by image name.
- Red-first on the parent and every mutation run in a scratch worktree: git worktree add --detach ${SP}wt-<stage> <sha>, built into the session's lane target directory; git worktree remove it when done. Never mutate ${A.wt} for a probe.
- Git: commit only on ${A.branch}; subjects start WP-xx: with a WP that docs/m0/authors.md §2 gives ${A.role}; explicit path lists after git diff --cached --stat. Never merge into ${A.trunk}, push, --force, --no-verify, -c core.hooksPath=, checkout --, restore, reset --hard, stash, clean or --amend.
- Save your report where your stage says and return the structured answer: verdict (or outcome), report path, head = the HEAD sha you examined.`
const S = (verdicts, extra) => ({
  type: 'object',
  properties: Object.assign({ verdict: { type: 'string', enum: verdicts }, report: { type: 'string' }, head: { type: 'string' } }, extra || {}),
  required: ['verdict', 'report', 'head'].concat(Object.keys(extra || {})),
})
const RATED = { critical: { type: 'integer' }, important: { type: 'integer' } }
const GATE = {
  type: 'object',
  properties: {
    outcome: { type: 'string', enum: ['PASS', 'FAIL', 'VACUOUS', 'HANG', 'VOID', 'LOCKFILE', 'STOPPED'] },
    report: { type: 'string' }, head: { type: 'string' }, tree: { type: 'string' }, kit: { type: 'string' },
  },
  required: ['outcome', 'report', 'head', 'tree', 'kit'],
}
const run = (label, ph, type, schema, prompt) => agent(`${HDR}\n${prompt}`, { label, phase: ph, agentType: type, schema })

phase('Cut')
const cut = await run('cut', 'Cut', 'architect', S(['CUT READY', 'STOPPED']),
  `ROLE: architect. First check ${A.spec} exists, else STOP. Write ${SP}cut.md: the spec re-located on this tree (file:line, only in paths ${A.role} may read); the WP and the commit subjects; the touch set (inside the lock set, writable by ${A.role}); the commits in order, each with its per-commit gates and its red-first (the predicted red); the expected test set by name (added, changed, removed); the questions only the orchestrator can decide (then verdict STOPPED).`)
if (!cut || cut.verdict !== 'CUT READY') return { ok: false, stage: 'cut', cut }
phase('Critique')
const crit = await run('critique', 'Critique', 'architecture-critic', S(['APPROVED', 'CHANGES_REQUESTED'], RATED),
  `ROLE: architecture-critic. First check ${SP}cut.md exists, else STOP. Critique it against the spec and the tree: wrong locations, a touch set outside the lock set or in another unit's set, a path ${A.role} may not write (docs/m0/authors.md §3) or read (deny_read), a gate that cannot fail, unproved claims, concurrency and performance hazards. Line-cited findings, each Critical, Important or Minor, and CONFIRMED or PLAUSIBLE. Save ${SP}critique.md.`)
if (!crit) return { ok: false, stage: 'critique', crit: null }
phase('Implement')
const impl = await run('implement', 'Implement', 'developer', S(['DONE', 'STOPPED']),
  `${A.resumeNote ? 'RESUME NOTE: ' + A.resumeNote + '\n' : ''}ROLE: developer, in ${A.wt}. First check ${SP}cut.md and ${SP}critique.md exist, else STOP. If ${A.trunk} moved (git merge-base --is-ancestor ${A.trunk} HEAD fails): git merge --no-ff ${A.trunk}; resolve a conflict only in a lock-set path, by editing and keeping both sides (a conflict in any other path is a STOP: never open it); then run the per-commit gates of this header, your role's own crates, and no other. Implement cut.md commit by commit. Resolve every Critical and Important of critique.md or refute it with evidence in an addendum at the end of cut.md; a Critical that changes the design is a STOP. Per commit: its red-first (the verdict line in the commit message body) and the per-commit gates for the crates it touches; the full gate is the mechanic's and the full suite the tester's. A manifest change that needs a new lockfile is a STOP with the reason "lockfile" (docs/m0/workflow.md section 4). Save ${SP}impl.md.`)
if (!impl || impl.verdict !== 'DONE') return { ok: false, stage: 'implement', crit: crit.verdict, impl }

const verify = (round, since) => {
  const delta = round > 1 || since !== A.base
  // Only the unit's own changes: across a trunk sync, git diff <since>..HEAD would show other roles' commits (S2).
  const scope = delta
    ? `DELTA since ${since}${round > 1 ? ' (' + SP + 'fix_r' + (round - 1) + '.md)' : ''}, the unit's own changes only: list them with git log --first-parent --format="%H %P %s" ${since}..HEAD; a commit with one parent is read with git show --name-only --format= <c>, then git show <c> -- <the lock-set paths>; a merge commit (a trunk sync) only through its conflict resolutions, git show --remerge-diff --name-only --format= <c>, then git show --remerge-diff <c> -- <the lock-set paths>. Never git diff ${since}..HEAD, git log -p or git show of a trunk commit: across a trunk sync they show other roles' changes. A listed path outside the lock set is a finding; never open it`
    : `the whole unit: git log ${A.trunk}..HEAD, git diff --name-only ${A.trunk}...HEAD, then git diff ${A.trunk}...HEAD -- <the lock-set paths>. A listed path outside the lock set is a finding; never open it`
  return parallel([
    async () => {
      const gate = await run(`gate-r${round}`, 'Verify', 'mechanic', GATE,
        `ROLE: mechanic, round ${round}. Run the full gate of ${A.branch} in ${A.gateWt} through the kit as your role file describes and summarise it; you give no verdict. Run directory ${SP}gate_r${round}; run id ${A.unit}-r${round}; -Detach ${A.branch}; -DiskFloorGb ${A.diskFloorGb}; command "cargo xtask gate --branch ${A.branch}". Save ${SP}gate_r${round}.md. Return the outcome (the verdict of DONE, or LOCKFILE when DONE says lockfile=updated) and head, tree and kit copied from DONE.`)
      if (!gate || (gate.outcome !== 'PASS' && gate.outcome !== 'FAIL')) return { gate, test: null }
      const test = await run(`test-r${round}`, 'Verify', 'tester', S(['GREEN', 'RED']),
        `ROLE: tester, round ${round}. First check ${SP}gate_r${round}.md exists, else STOP; check it against the kit's own files in ${SP}gate_r${round}/ (DONE, summary.tsv) and record any disagreement as a mechanic escape in your report. Scope: ${scope}. Every red-first re-run red on the parent and green on HEAD; at least 2 mutations of your own (at the change and at any gate this round added) that a named gate must catch; the pins; the tests of the crates the unit touches, run through the kit as your role file gives (-RunDir ${SP}tests_r${round}-<m> and -RunId ${A.unit}-t${round}-<m>, m = 1, 2, ... per run this round; -Kind tests; -Expected ${SP}expected_r${round}.tsv, which you write from cut.md's expected set and ${SP}baseline_unit.tsv, never from ${A.orch}/baseline.tsv), counted BY NAME (zero tests = RED). A gate outcome other than PASS is RED. Never commit, never edit ${A.wt}. Save ${SP}test_r${round}.md.`)
      return { gate, test }
    },
    () => run(`review-r${round}`, 'Verify', 'code-reviewer', S(['APPROVED', 'CHANGES_REQUESTED'], RATED),
      `ROLE: code-reviewer, round ${round}. Review ${scope}${round === 1 ? '; and the addendum of ' + SP + 'cut.md' : '; and whether it closes the CONFIRMED items of ' + SP + 'triage_r' + (round - 1) + '.md'}. Read code through git show and git diff in ${A.wt}, only paths ${A.role} may read. Check that every path the scope lists (the unit's commits and a merge's conflict resolutions) is in the lock set and writable by ${A.role} (docs/m0/authors.md §3) and that every commit subject except a trunk sync's names a WP of the role. Line-cited defects only, each Critical, Important or Minor, and CONFIRMED or PLAUSIBLE. Save ${SP}review_r${round}.md.`),
  ]).then(([arm, review]) => ({ gate: arm && arm.gate, test: arm && arm.test, review, scope }))
}
const green = v => v.gate && v.gate.outcome === 'PASS' && v.test && v.test.verdict === 'GREEN' &&
  v.review && v.review.critical + v.review.important === 0
// A stage that returned nothing, or a gate that did not reach a verdict, goes back to the orchestrator.
const broken = v => {
  if (!v.gate) return 'gate: the mechanic returned nothing'
  if (v.gate.outcome !== 'PASS' && v.gate.outcome !== 'FAIL') return 'gate: ' + v.gate.outcome
  if (!v.test) return 'test: the tester returned nothing'
  if (!v.review) return 'review: the reviewer returned nothing'
  return ''
}

let v = await verify(1, SINCE0 || A.base)
let round = 1
while (!green(v) && !broken(v) && round < 3) {
  phase('Fix')
  const tri = await run(`triage-r${round}`, 'Fix', 'code-reviewer', S(['DONE'], { confirmed: { type: 'integer' } }),
    `ROLE: code-reviewer as the adversarial verifier (triage), read-only except scratch probes under ${SP}. Try to REFUTE every red in ${SP}test_r${round}.md, a FAIL in ${SP}gate_r${round}.md and every Critical and Important in ${SP}review_r${round}.md, against HEAD, reading code only within the round's scope: ${v.scope}. Per item: CONFIRMED, REFUTED or OUT-OF-SCOPE, with the reason. Save ${SP}triage_r${round}.md.`)
  if (!tri || tri.confirmed === 0) return { ok: false, stage: 'triage', note: 'nothing confirmed: the orchestrator rules on any remaining red', gate: v.gate, test: v.test, review: v.review, tri, reports: SP }
  const since = v.gate.head
  const fix = await run(`fix-r${round}`, 'Fix', 'developer', S(['DONE', 'STOPPED']),
    `ROLE: developer, in ${A.wt}. Fix the CONFIRMED items of ${SP}triage_r${round}.md as follow-up commits (never amend), each red-first; re-run the per-commit gates the fix touches. Save ${SP}fix_r${round}.md.`)
  if (!fix || fix.verdict !== 'DONE') return { ok: false, stage: 'fix', round, fix, reports: SP }
  round++
  v = await verify(round, since)
}
const why = broken(v)
return {
  ok: !!green(v), unit: A.unit, stage: why ? why.split(':')[0] : 'verify', note: why, round, crit: crit.verdict,
  gate: v.gate && v.gate.outcome, head: v.gate && v.gate.head, tree: v.gate && v.gate.tree, kit: v.gate && v.gate.kit,
  test: v.test && v.test.verdict, review: v.review && v.review.verdict,
  critical: v.review && v.review.critical, important: v.review && v.review.important, reports: SP,
}
