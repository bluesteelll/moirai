export const meta = {
  name: 'unit',
  description: 'One moirai unit on its role branch, by tier: HEAVY cut -> critique -> implement -> (mechanic gate -> tester) || review -> triage -> fix; STANDARD implement (own cut) -> gate -> verifier -> fix; LIGHT implement -> gate || review -> fix; delta re-verify; local commits only',
  whenToUse: 'Launched by the orchestrator session with the Workflow tool, from the embedded copy, as docs/m0/workflow.md section 6.3 describes',
  phases: [ { title: 'Cut' }, { title: 'Critique' }, { title: 'Implement' }, { title: 'Verify' }, { title: 'Fix' } ],
}
// The unit workflow of docs/m0/workflow.md section 6, launched by the orchestrator session (RULINGS R-8, R-9).
// Arguments (args, or embedded by <ORCH>/kit/embed.ps1, which replaces the line below in the per-launch copy
// <ORCH>/embed/<unit>.js; docs/m0/workflow.md section 6.2):
//   role (author role, xtask/roles.toml), unit, orch, wt (the role worktree), branch (m0/<role>), base (the master sha
//   the unit started from), trunk (master), spec (a path), lock and others (repository paths only; named sections go
//   to <ORCH>/<unit>/lock.md), gates (the path <ORCH>/<unit>/gates.md), tier (LIGHT, STANDARD or HEAVY:
//   docs/m0/workflow.md section 5.1), class (M, MD, R, N, D, B, V or L: section 5.1), extras ('none' or
//   '<ORCH>/<unit>/extras.tsv: <ids>'), gateCmd (the full gate's command), unitTomlSha (the sha256 of
//   <ORCH>/<unit>/unit.toml, the kit config that holds the target directory, the jobs, the test threads, the gate
//   worktree and the disk floor), resumeNote, dryRun; optional gateRun (a token: change it on a relaunch that must
//   re-run the gate), deltaLogic (read only with a DELTA SINCE resumeNote; false says that the delta changed no
//   non-test code, which makes the first round slim; default true) and deltaRound (read only with a DELTA SINCE
//   resumeNote: the round the first verify round is; default 1). Old-form args (gateWt, diskFloorGb) are refused.
// A resumeNote reaches only the implement stage, so a relaunch replays the cut and the critique from the journal.
// A resumeNote that starts "DELTA SINCE <sha>:" makes the first verify round delta-scoped since that verified head:
// round 1, or round n with deltaRound n, which a gate re-run of round n of 2 or more passes so that the relaunch
// repeats round n with its closure checks and its place in the round limit (docs/m0/workflow.md section 6.4).
// A HEAVY delta round whose delta changed no non-test code (logic false) is slim: the gate, then the tester, who
// checks that flag; the review runs after the tester only when the tester finds the flag wrong. Otherwise the review
// starts first, so a relaunch that changes only gateRun replays round 1's review from the journal. That relaunch is
// for round 1's gate only (docs/m0/workflow.md section 6.4): a round whose review examined another head than its gate
// verified goes back to the orchestrator.
const A = args || {} // EMBED: kit/embed.ps1 replaces this line in the per-launch copy

const NEED = ['role', 'unit', 'orch', 'wt', 'branch', 'base', 'trunk', 'spec', 'lock', 'others', 'gates', 'tier', 'class', 'extras', 'gateCmd', 'unitTomlSha']
const refuse = error => ({ ok: false, stage: 'args', error })
if (A.gateWt !== undefined || A.diskFloorGb !== undefined) return refuse('unconverted args: gateWt and diskFloorGb moved into <ORCH>/<unit>/unit.toml (docs/m0/workflow.md section 6.2)')
const missing = NEED.filter(k => A[k] === undefined || A[k] === null || A[k] === '')
if (missing.length) return { ok: false, stage: 'args', missing, error: 'missing args: ' + missing.join(' ') }
if (A.branch !== 'm0/' + A.role) return refuse('branch must be m0/<role>')
if (!['LIGHT', 'STANDARD', 'HEAVY'].includes(A.tier)) return refuse('tier must be LIGHT, STANDARD or HEAVY')
if (!['M', 'MD', 'R', 'N', 'D', 'B', 'V', 'L'].includes(A.class)) return refuse('class must be M, MD, R, N, D, B, V or L (docs/m0/workflow.md section 5.1)')
if (A.class === 'M' && A.tier !== 'HEAVY') return refuse('class M needs tier HEAVY (a map rename stays HEAVY)')
const esc = t => String(t).replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
if (!new RegExp('^cargo xtask gate --branch ' + esc(A.branch) + '( --range [^ ]+\\.\\.' + esc(A.branch) + ')?$').test(A.gateCmd)) return refuse('gateCmd must be "cargo xtask gate --branch <branch>", optionally with " --range <base>..<branch>"')
// The stages and the round limit of each tier (docs/m0/workflow.md sections 5.1 and 6.1). The dry run returns them,
// and a stage runs only when its tier lists it.
const TIERS = {
  HEAVY: { stages: ['cut', 'critique', 'implement', 'gate', 'test', 'review', 'triage', 'fix'], maxRounds: 3 },
  STANDARD: { stages: ['implement', 'gate', 'verifier', 'fix'], maxRounds: 2 },
  LIGHT: { stages: ['implement', 'gate', 'review', 'fix'], maxRounds: 2 },
}
const T = TIERS[A.tier]
const DM = /^DELTA SINCE ([0-9a-f]{7,40})\b/.exec(A.resumeNote || '')
const SINCE0 = DM ? DM[1] : ''
const GR = /^DELTA SINCE [0-9a-f]{7,40}: gate re-run of round (\d+)\b/.exec(A.resumeNote || '')
const R0 = A.deltaRound === undefined || A.deltaRound === null || A.deltaRound === '' ? 1 : A.deltaRound
if (!Number.isInteger(R0) || R0 < 1 || R0 > T.maxRounds || (R0 > 1 && !DM) || (GR && Number(GR[1]) !== R0)) return { ok: false, stage: 'args', error: 'deltaRound must be a round of the tier (default 1), above 1 only with a DELTA SINCE resumeNote, and n with a "gate re-run of round <n>" resumeNote' }
const has = s => T.stages.includes(s)
const HEAVY = A.tier === 'HEAVY'

const SP = A.orch + '/' + A.unit + '/'
const TIER = HEAVY ? '- Tier HEAVY (docs/m0/workflow.md section 5.1).'
  : `- Tier ${A.tier} (docs/m0/workflow.md section 5.1). The HEAVY triggers: H1 product source, a product crate's manifest or build.rs, or the build configuration; H2 the simulator or the toy log; H3 bytes, ids, hashes, codecs or the canonical form; H4 what a rule requires; H5 a decision the spec leaves open, or a ruling applied for the first time; H6 the merge-verdict machinery (unit.js control flow and schemas, the kit, .githooks, the gate, the fmt, clippy and mutation configuration, the by-name and zero-test rules). A trigger met in the spec, the tree or the diff is a STOP with the reason "escalate: H<n>"; when in doubt, the higher tier.${A.tier === 'LIGHT' ? ' A change of behaviour or of a test is a STOP with the reason "escalate: STANDARD".' : ''}`
// The kit's cargo wrapper with the unit config (K2): every cargo call of a unit agent goes through it, so the target
// directory, the jobs and the test threads are unit.toml's, never the session's.
const K2 = `powershell.exe -NoProfile -ExecutionPolicy Bypass -File ${A.orch}/kit/cargo.ps1 -UnitConfig ${SP}unit.toml -UnitConfigSha256 ${A.unitTomlSha}`
// The header every prompt starts with (docs/m0/workflow.md section 6.1): at most 5,000 characters, refused above.
const HDR = [
  `UNIT ${A.unit}, author role ${A.role}, class ${A.class}, tier ${A.tier}. Spec: ${A.spec}. Rulings: ${SP}rulings.md, cited by id. Procedure: docs/m0/workflow.md.`,
  `- Author role ${A.role} binds you: PLAN §3.1 (S1-S6) and the write map of docs/m0/authors.md §3.`,
  `- Reading limit: your role file's section "Reading limit", row ${A.role}; extras: ${A.extras}. It binds every way of reading, Bash included (cat, rg, git show <rev>:<path>, git diff over those paths); a file you need but may not read under it is a finding in your report, never a read; the rows of ${SP}extras.tsv that extras names are your extras: limits, or the S/A lines you may read.`,
  `- Every shell command starts with cd into your directory: ${A.wt} unless your stage names another. Write only in ${A.wt}, under ${SP} and in the scratch worktrees your stage makes; delete only by absolute path under ${SP}. Read-only to you: ${SP}unit.toml, ${SP}expected-red.tsv, ${SP}gates.md and ${SP}extras.tsv.`,
  `- Red-first on the parent and every mutation run in a scratch worktree: git worktree add --detach ${SP}wt-<stage> <sha>, built only through cargo.ps1 as the per-commit line below gives; git worktree remove it when done. Never mutate ${A.wt} for a probe.`,
  `- Branch ${A.branch} from ${A.base}; trunk ${A.trunk}. Lock set: ${A.lock}. Other open units: ${A.others}. Their named sections: ${SP}lock.md. A file outside the lock set, or in another unit's set, is a STOP.`,
  TIER,
  `- Per-commit gates: ${A.gates}; every cargo call there runs through ${K2} -WorkDir <tree> "<cargo arguments>".`,
  `- The full verdict is ${A.gateCmd}, run by the mechanic through ${A.orch}/kit; the mechanic reports a kit refusal of it as STOPPED, never RED.`,
  `- Long runs go through ${A.orch}/kit (start-gate.ps1, wait-gate.ps1, stop-gate.ps1) under a budget; wait in bounded slices; past the budget is a hang: stop it by its own PID tree, never by image name.`,
  `- Git: commit only on ${A.branch}; subjects start WP-xx: with a WP that docs/m0/authors.md §2 gives ${A.role}; explicit path lists after git diff --cached --stat. Never merge into ${A.trunk}, push, --force, --no-verify, -c core.hooksPath=, checkout --, restore, reset --hard, stash, clean or --amend.`,
  `- Save your report where your stage says and return the structured answer: verdict (or outcome), report path, head = the HEAD sha you examined.`,
].join('\n') // HDR
if (HDR.length > 5000) return refuse(`header ${HDR.length} characters, above 5,000: shorten lock or others (named sections go to ${SP}lock.md; docs/m0/workflow.md section 5.2)`)
if (A.dryRun) return { ok: true, args: A, stages: T.stages.slice(), maxRounds: T.maxRounds, hdrChars: HDR.length }
const S = (verdicts, extra) => ({
  type: 'object',
  properties: Object.assign({ verdict: { type: 'string', enum: verdicts }, report: { type: 'string' }, head: { type: 'string' } }, extra || {}),
  required: ['verdict', 'report', 'head'].concat(Object.keys(extra || {})),
})
const RATED = { critical: { type: 'integer' }, important: { type: 'integer' } }
// Below HEAVY there is no triage: critical and important count CONFIRMED items only, and a PLAUSIBLE Critical or
// Important (plausible_ci) blocks ok for the orchestrator to rule.
const RATED_P = Object.assign({}, RATED, { plausible_ci: { type: 'integer' } })
const BOOL = { type: 'boolean' }
const GATE = {
  type: 'object',
  properties: {
    outcome: { type: 'string', enum: ['PASS', 'FAIL', 'VACUOUS', 'HANG', 'VOID', 'LOCKFILE', 'STOPPED'] },
    report: { type: 'string' }, head: { type: 'string' }, tree: { type: 'string' }, kit: { type: 'string' },
    rows: { type: 'integer' },
  },
  required: ['outcome', 'report', 'head', 'tree', 'kit', 'rows'],
}
// Reasoning effort per role, the same values as the role files' `effort` (docs/m0/workflow.md section 9). Passed per
// call, because a workflow agent with no `effort` inherits the session's.
const EFFORT = { architect: 'xhigh', 'architecture-critic': 'xhigh', 'code-reviewer': 'xhigh', verifier: 'xhigh',
  tester: 'xhigh', developer: 'high', mechanic: 'high' }
const run = (label, ph, type, schema, prompt) =>
  agent(`${HDR}\n${prompt}`, { label, phase: ph, agentType: type, schema, effort: EFFORT[type] })

let crit = null
if (has('cut')) {
  phase('Cut')
  const cut = await run('cut', 'Cut', 'architect', S(['CUT READY', 'STOPPED']),
    `ROLE: architect. First check ${A.spec} exists, else STOP. Write ${SP}cut.md: the spec re-located on this tree (file:line, only in paths ${A.role} may read); the WP and the commit subjects; the touch set (inside the lock set, writable by ${A.role}); the commits in order, each with its per-commit gates and its red-first (the predicted red); the expected test set by name (added, changed, removed); the questions only the orchestrator can decide (then verdict STOPPED).`)
  if (!cut || cut.verdict !== 'CUT READY') return { ok: false, tier: A.tier, stage: 'cut', cut }
}
if (has('critique')) {
  phase('Critique')
  crit = await run('critique', 'Critique', 'architecture-critic', S(['APPROVED', 'CHANGES_REQUESTED'], RATED),
    `ROLE: architecture-critic. First check ${SP}cut.md exists, else STOP. Critique it against the spec and the tree: wrong locations, a touch set outside the lock set or in another unit's set, a path ${A.role} may not write (docs/m0/authors.md §3), a read the spec's reading limits forbid, a gate that cannot fail, unproved claims, concurrency and performance hazards. Line-cited findings, each Critical, Important or Minor, and CONFIRMED or PLAUSIBLE. Save ${SP}critique.md.`)
  if (!crit) return { ok: false, tier: A.tier, stage: 'critique', crit: null }
}
const CRIT = crit ? crit.verdict : 'none'
phase('Implement')
const SYNC = `If ${A.trunk} moved (git merge-base --is-ancestor ${A.trunk} HEAD fails): git merge --no-ff ${A.trunk}; resolve a conflict only in a lock-set path, by editing and keeping both sides (a conflict in any other path is a STOP: never open it); then run the per-commit gates of this header, your role's own crates, and no other.`
const PER = `Per commit: its red-first (the verdict line in the commit message body) and the per-commit gates for the crates it touches; the full gate is the mechanic's and the full suite the tester's or the verifier's. A manifest change that needs a new lockfile is a STOP with the reason "lockfile" (docs/m0/workflow.md section 4). Save ${SP}impl.md.`
const START = {
  HEAVY: `First check ${SP}cut.md and ${SP}critique.md exist, else STOP. ${SYNC} Implement cut.md commit by commit. Resolve every Critical and Important of critique.md or refute it with evidence in an addendum at the end of cut.md; a Critical that changes the design is a STOP. ${PER}`,
  STANDARD: `First check ${A.spec} exists, else STOP. ${SYNC} Before any edit write ${SP}cut.md yourself, at most about 60 lines (a relaunch that finds it keeps it and adds a dated addendum at its end): first the line "Triggers H1-H6: none" (a trigger met is a STOP with the reason "escalate: H<n>"); the touch set inside the lock set; the commits in order, each with its per-commit gates and its red-first; the expected test set by name (added, changed, removed); the unit's main red-first and the mutation at the change for the verifier; a harness that judges an exit criterion or a GT gate is also shown red on a seeded fault (docs/m0/workflow.md section 5.5, S4). Then implement it commit by commit. ${PER}`,
  LIGHT: `First check ${A.spec} exists, else STOP. ${SYNC} Make the change the spec lists. It changes no behaviour and adds, removes or renames no test; if it would, STOP with the reason "escalate: STANDARD". There is no red-first (no behavioural claim). Per commit: the per-commit gates for the crates it touches; the full gate is the mechanic's and the full suite the tester's or the verifier's. A manifest change that needs a new lockfile is a STOP with the reason "lockfile" (docs/m0/workflow.md section 4). In ${SP}impl.md list the changed paths and state "no behaviour or test change". Save ${SP}impl.md.`,
}
const impl = await run('implement', 'Implement', 'developer', S(['DONE', 'STOPPED']),
  `${A.resumeNote ? 'RESUME NOTE: ' + A.resumeNote + '\n' : ''}ROLE: developer, in ${A.wt}. ${START[A.tier]}`)
if (!impl || impl.verdict !== 'DONE') return { ok: false, tier: A.tier, stage: 'implement', crit: CRIT, impl }

// The gate run token (U4): in the gate's, tester's, verifier's, triage's and fix's prompts, never the review's, so a
// relaunch that changes it re-runs the gate and every checker after it while round 1's review replays (section 6.4).
const GRT = A.gateRun ? ' Gate run ' + A.gateRun + '.' : ''
const GF = { type: 'string', enum: ['MATCH', 'MISMATCH'] }
// The mechanic's fields, which the tester or the verifier checks against the kit's own files (U4, R-164 ruling 5).
const FIELDS = (g, round) => ` The mechanic returned: outcome ${g.outcome}, head ${g.head}, tree ${g.tree}, kit ${g.kit}, rows ${g.rows}; the gate command: ${A.gateCmd}. Return gate_fields MATCH when those equal ${SP}gate_r${round}/DONE and summary.tsv's row count, ${SP}gate_r${round}/cmd.txt (one word per line) is the gate command word for word, and the log's range line is the range the command implies (master..${A.branch}, or its --range); else MISMATCH.`
const COUNTS = ' Return critical and important as the counts of CONFIRMED Critical and Important items only, and plausible_ci as the count of PLAUSIBLE Critical and Important items; list every item in the report.'
const verify = async (round, since, logic) => {
  const delta = round > 1 || since !== A.base
  const slim = !logic
  const prev = round - 1
  // Only the unit's own changes: across a trunk sync, git diff <since>..HEAD would show other roles' commits (S2).
  const scope = delta
    ? `DELTA since ${since}${round > 1 ? ' (' + SP + 'fix_r' + (round - 1) + '.md)' : ''}, the unit's own changes only: list them with git log --first-parent --format="%H %P %s" ${since}..HEAD; a commit with one parent is read with git show --name-only --format= <c>, then git show <c> -- <the lock-set paths>; a merge commit (a trunk sync) only through its conflict resolutions, git show --remerge-diff --name-only --format= <c>, then git show --remerge-diff <c> -- <the lock-set paths>. Never git diff ${since}..HEAD, git log -p or git show of a trunk commit: across a trunk sync they show other roles' changes. A listed path outside the lock set is a finding; never open it`
    : `the whole unit: git log ${A.trunk}..HEAD, git diff --name-only ${A.trunk}...HEAD, then git diff ${A.trunk}...HEAD -- <the lock-set paths>. A listed path outside the lock set is a finding; never open it`
  const gateArm = () => run(`gate-r${round}`, 'Verify', 'mechanic', GATE,
    `ROLE: mechanic, round ${round}.${GRT} Run the full gate of ${A.branch} through the kit as your role file describes and summarise it; you give no verdict. From ${SP}: powershell.exe -NoProfile -ExecutionPolicy Bypass -File ${A.orch}/kit/start-gate.ps1 -UnitConfig ${SP}unit.toml -UnitConfigSha256 ${A.unitTomlSha} -RunDir ${SP}gate_r${round} -Unit ${A.unit} -RunId ${A.unit}-r${round} -Kind gate -Detach ${A.branch} -CmdLine "${A.gateCmd}" (the config gives the gate worktree, the target directory, the jobs and the floor). Save ${SP}gate_r${round}.md. Return the outcome (the verdict of DONE, or LOCKFILE when DONE says lockfile=updated), head, tree and kit copied from DONE, and rows, the number of summary.tsv rows. For a crate whose failure text your reading limit withholds, report its FAILED ids, count lines and log line numbers only.`)
  const reached = g => !!g && (g.outcome === 'PASS' || g.outcome === 'FAIL')
  const gateCheck = `First check ${SP}gate_r${round}.md exists, else STOP; check it against the kit's own files in ${SP}gate_r${round}/ (DONE, summary.tsv) and record any disagreement as a mechanic escape in your report.`
  const tests = how => `the tests of the crates the unit touches, run through the kit as ${how} (-UnitConfig ${SP}unit.toml -UnitConfigSha256 ${A.unitTomlSha}; -RunDir ${SP}tests_r${round}-<m> and -RunId ${A.unit}-t${round}-<m>, m = 1, 2, ... per run this round; -Kind tests; -Expected ${SP}expected_r${round}.tsv, which you write from cut.md's expected set and ${SP}baseline_unit.tsv, never from ${A.orch}/baseline.tsv), counted BY NAME (zero tests = RED)`
  const redo = `every red-first of the delta's commits re-run red on the parent and green on HEAD${round > 1 ? '; re-run each mutation that survived round ' + prev : ''}`
  const flag = `the delta is reported to change no non-test code (the fix agent's logic flag, or the orchestrator's deltaLogic): check that flag against git show of each of the delta's commits (every changed hunk must lie in a test module, a test file or a comment); if one does not, rate "logic flag wrong" an Important CONFIRMED in your report (the fix agent's escape) and make one mutation of your own at a line the delta changed`
  const LIGHTCHK = A.tier !== 'LIGHT' ? '' : ' This LIGHT unit has no other checker: re-answer the triggers H1-H6 on the diff (a trigger met is a Critical CONFIRMED with the reason "escalate: H<n>"); check that the diff changes no behaviour and adds, removes or renames no test (otherwise a Critical CONFIRMED with the reason "escalate: STANDARD"); and check that every changed sentence that says what code does matches the code it cites.'
  const close = HEAVY
    ? (round === 1 ? '; and the addendum of ' + SP + 'cut.md' : '; and whether it closes the CONFIRMED items of ' + SP + 'triage_r' + prev + '.md')
    : (round === 1 ? '' : '; and whether it closes the CONFIRMED Critical and Important items of ' + SP + 'review_r' + prev + '.md')
  const reviewArm = () => run(`review-r${round}`, 'Verify', 'code-reviewer', S(['APPROVED', 'CHANGES_REQUESTED'], HEAVY ? RATED : RATED_P),
    `ROLE: code-reviewer, round ${round}. Review ${scope}${close}. Read code through git show and git diff in ${A.wt}, only paths ${A.role} may read. Check that every path the scope lists (the unit's commits and a merge's conflict resolutions) is in the lock set and writable by ${A.role} (docs/m0/authors.md §3) and that every commit subject except a trunk sync's names a WP of the role. Line-cited defects only, each Critical, Important or Minor, and CONFIRMED or PLAUSIBLE.${HEAVY ? '' : COUNTS}${LIGHTCHK} Save ${SP}review_r${round}.md.`)

  if (has('test')) {
    const muts = !delta
      ? 'Every red-first re-run red on the parent and green on HEAD; at least 2 mutations of your own (at the change and at any gate this round added) that a named gate must catch'
      : logic ? `Then ${redo}; 1 new mutation of your own at a line the delta changed that a named gate must catch`
      : `Then ${redo}; no new mutation: ${flag}, and return logic = true (else logic = false); the flag alone does not make your verdict RED, since its mutation ran in this round`
    const testArm = async () => {
      const gate = await gateArm()
      if (!reached(gate)) return { gate, test: null }
      const test = await run(`test-r${round}`, 'Verify', 'tester', S(['GREEN', 'RED'], Object.assign({ gate_fields: GF }, slim ? { logic: BOOL } : {})),
        `ROLE: tester, round ${round}.${GRT} ${gateCheck}${FIELDS(gate, round)} Expected-red set: ${A.expectedRed ? A.expectedRed + ', sha256 ' + A.expectedRedSha + '; check the file against that sha' : 'none'}. Scope: ${scope}. ${muts}; the pins; ${tests('your role file gives')}. A gate outcome other than PASS is RED. Never commit, never edit ${A.wt}. Save ${SP}test_r${round}.md.`)
      return { gate, test }
    }
    if (!slim) {
      const [review, arm] = await parallel([reviewArm, testArm])
      return { gate: arm && arm.gate, test: arm && arm.test, review, reviewDue: true, slim, scope }
    }
    const arm = await testArm()
    const reviewDue = !!arm.test && arm.test.logic === true
    const review = reviewDue ? await reviewArm() : null
    return { gate: arm.gate, test: arm.test, review, reviewDue, slim, scope }
  }
  if (has('verifier')) {
    const gate = await gateArm()
    if (!reached(gate)) return { gate, ver: null, slim, scope }
    const muts = !delta
      ? `re-run the main red-first that ${SP}cut.md marks, red on the parent and green on HEAD; 1 mutation of your own at a line the unit changed that a named test must catch`
      : logic ? `${redo}; 1 new mutation of your own at a line the delta changed that a named test must catch`
      : `${redo}; no new mutation: ${flag}; that item is not counted in critical or important, since its mutation ran in this round`
    const ver = await run(`verifier-r${round}`, 'Verify', 'verifier', S(['GREEN', 'RED'], Object.assign({ gate_fields: GF }, RATED_P)),
      `ROLE: verifier, round ${round}.${GRT} ${gateCheck}${FIELDS(gate, round)} Scope: ${scope}. Review the scope as your role file gives${round === 1 ? '' : ', and whether the fix closes the CONFIRMED Critical and Important items of ' + SP + 'verify_r' + prev + '.md'}. Then ${muts}; the pins; the developer's expected set in ${SP}cut.md against the tests the diff removes; ${tests('tester.md "The tests of a round" gives')}. A gate FAIL is the unit's when its row is of a crate the unit touches or of a crate that depends on a changed one; any other FAIL is environmental only on R-10's terms (docs/m0/workflow.md section 10). Line-cited findings, each Critical, Important or Minor, and CONFIRMED or PLAUSIBLE; a surviving mutation, a missing expected test, a by-name mismatch or a red-first that is not red is an Important CONFIRMED.${COUNTS} A gate outcome other than PASS is RED. Never commit, never edit ${A.wt}. Save ${SP}verify_r${round}.md.`)
    return { gate, ver, slim, scope }
  }
  const [review, gate] = await parallel([reviewArm, gateArm])
  return { gate, review, reviewDue: true, slim: false, scope }
}
const ok0 = r => !!r && r.critical + r.important === 0
const okP = r => ok0(r) && r.plausible_ci === 0
const green = v => !!v.gate && v.gate.outcome === 'PASS' && (has('test')
  ? !!v.test && v.test.verdict === 'GREEN' && (!v.reviewDue || ok0(v.review))
  : has('verifier') ? !!v.ver && v.ver.verdict === 'GREEN' && okP(v.ver) : okP(v.review))
// A review answers for the head it examined; a short sha is the same head, and an empty one is none.
const norm = h => String(h || '').trim().toLowerCase()
const sameHead = (a, b) => { const x = norm(a), y = norm(b); return x !== '' && y !== '' && (x === y || (x.length >= 7 && y.startsWith(x)) || (y.length >= 7 && x.startsWith(y))) }
// A stage that returned nothing, a gate that did not reach a verdict, or a review of another head than the gate
// verified (a review replayed from an earlier launch, docs/m0/workflow.md section 6.4) goes back to the orchestrator.
const broken = v => {
  if (!v.gate) return 'gate: the mechanic returned nothing'
  if (v.gate.outcome !== 'PASS' && v.gate.outcome !== 'FAIL') return 'gate: ' + v.gate.outcome
  if (has('test') && !v.test) return 'test: the tester returned nothing'
  if (has('verifier') && !v.ver) return 'verifier: the verifier returned nothing'
  if (v.reviewDue && !v.review) return 'review: the reviewer returned nothing'
  // The tester's or the verifier's check of the mechanic's fields against DONE (U4).
  if ((v.test && v.test.gate_fields === 'MISMATCH') || (v.ver && v.ver.gate_fields === 'MISMATCH')) return 'gate: mechanic fields disagree with DONE'
  if (v.review && !sameHead(v.review.head, v.gate.head)) return `review: the review examined ${norm(v.review.head) || 'no head'} and the gate verified ${v.gate.head}: not a review of the verified head (one replayed from an earlier launch, or a misreported head); after a gateRun-only relaunch of round n's gate, relaunch DELTA SINCE the head gate_r<n-1>.md verified, with deltaRound n (docs/m0/workflow.md sections 5.3 and 6.4)`
  return ''
}

let round = R0
let v = await verify(round, SINCE0 || A.base, SINCE0 ? A.deltaLogic !== false : true)
while (!green(v) && !broken(v) && round < T.maxRounds) {
  phase('Fix')
  let items
  if (has('triage')) {
    const tri = await run(`triage-r${round}`, 'Fix', 'code-reviewer', S(['DONE'], { confirmed: { type: 'integer' } }),
      `ROLE: code-reviewer as the adversarial verifier (triage).${GRT} Read-only except scratch probes under ${SP}. Try to REFUTE every red in ${SP}test_r${round}.md, a FAIL in ${SP}gate_r${round}.md${v.review ? ' and every Critical and Important in ' + SP + 'review_r' + round + '.md' : ' (a slim round: no review ran)'}, against HEAD, reading code only within the round's scope: ${v.scope}. Per item: CONFIRMED, REFUTED or OUT-OF-SCOPE, with the reason. Save ${SP}triage_r${round}.md.`)
    if (!tri || tri.confirmed === 0) return { ok: false, tier: A.tier, stage: 'triage', note: 'nothing confirmed: the orchestrator rules on any remaining red', gate: v.gate, test: v.test, review: v.review, tri, reports: SP }
    items = `the CONFIRMED items of ${SP}triage_r${round}.md`
  } else {
    // Below HEAVY the checker's CONFIRMED rating is the confirmation; anything else is the orchestrator's to rule.
    const rep = has('verifier') ? v.ver : v.review
    if (rep.critical + rep.important === 0) {
      return {
        ok: false, unit: A.unit, tier: A.tier, class: A.class, stage: v.gate.outcome !== 'PASS' ? 'gate' : has('verifier') ? 'verifier' : 'review',
        note: 'no CONFIRMED Critical or Important: the orchestrator rules (a PLAUSIBLE item, or an environmental gate FAIL under R-10)',
        round, crit: CRIT, gate: v.gate.outcome, head: v.gate.head, tree: v.gate.tree, kit: v.gate.kit, rows: v.gate.rows,
        ver: v.ver && v.ver.verdict, review: v.review && v.review.verdict,
        critical: rep.critical, important: rep.important, plausible: rep.plausible_ci, slim: !!v.slim, reports: SP,
      }
    }
    items = `the CONFIRMED Critical and Important items of ${SP}${has('verifier') ? 'verify' : 'review'}_r${round}.md`
  }
  const since = v.gate.head
  const fix = await run(`fix-r${round}`, 'Fix', 'developer', S(['DONE', 'STOPPED'], { logic: BOOL }),
    `ROLE: developer, in ${A.wt}.${GRT} Fix ${items} as follow-up commits (never amend), each behavioural fix red-first; re-run the per-commit gates the fix touches. Refute an item you find wrong with evidence in ${SP}fix_r${round}.md instead of fixing it. An item marked "escalate"${HEAVY ? '' : ', or one whose fix would meet a HEAVY trigger,'} is a STOP with the reason "escalate: <trigger>", never a fix. Return logic = true when any fix commit changed non-test, non-comment code, and state it in ${SP}fix_r${round}.md. Save ${SP}fix_r${round}.md.`)
  if (!fix || fix.verdict !== 'DONE') return { ok: false, tier: A.tier, stage: 'fix', round, fix, reports: SP }
  round++
  v = await verify(round, since, fix.logic !== false)
}
const why = broken(v)
const rep = has('verifier') ? v.ver : v.review
return {
  ok: !why && !!green(v), unit: A.unit, tier: A.tier, class: A.class, stage: why ? why.split(':')[0] : 'verify', note: why, round, crit: CRIT,
  gate: v.gate && v.gate.outcome, head: v.gate && v.gate.head, tree: v.gate && v.gate.tree, kit: v.gate && v.gate.kit, rows: v.gate && v.gate.rows,
  test: v.test && v.test.verdict, ver: v.ver && v.ver.verdict, review: v.review && v.review.verdict,
  critical: rep && rep.critical, important: rep && rep.important, plausible: rep && rep.plausible_ci, slim: !!v.slim, reports: SP,
}
