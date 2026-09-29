//! The rule tables of [RULES/README] as data ([60 §4.2], [60 §4.5]; `docs/m0/PLAN.md` §3.2 item 9): every rule file
//! is included with `include_str!` from `docs/spec/rules/`, the only copy of the signed bytes ([RULES/README] §8, its
//! open point 1), parsed once per process behind a [`OnceLock`] by the hand-written parser of [`parse`], checked
//! against the registry of [RULES/README] §7 and the column lists the model implements ([`IMPLEMENTED`]), and
//! referentially checked (§8). Any violation panics naming the file, the line, the table and the column.
//!
//! The consumers read tables by id ([`Rules::table`]) and evaluate them as data: decision tables in row order (first
//! match), allowlists by "some row matches", procedure tables by one tagged function per row.

pub mod parse;

use parse::{Cell, ColType, Column, ParseError};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// The rule files the model parses, by name, with their bytes ([RULES/README] §1.1; `SIGNED.md` is read at run time
/// by [`signed_rows`], §6).
pub const FILES: [(&str, &str); 9] = [
    (
        "README.md",
        include_str!("../../../docs/spec/rules/README.md"),
    ),
    (
        "merge-table.md",
        include_str!("../../../docs/spec/rules/merge-table.md"),
    ),
    (
        "link-merge-rules.md",
        include_str!("../../../docs/spec/rules/link-merge-rules.md"),
    ),
    (
        "pack-classes.md",
        include_str!("../../../docs/spec/rules/pack-classes.md"),
    ),
    (
        "role-write-policy.md",
        include_str!("../../../docs/spec/rules/role-write-policy.md"),
    ),
    (
        "state-definition.md",
        include_str!("../../../docs/spec/rules/state-definition.md"),
    ),
    (
        "status-machines.md",
        include_str!("../../../docs/spec/rules/status-machines.md"),
    ),
    (
        "delete-policy-matrix.md",
        include_str!("../../../docs/spec/rules/delete-policy-matrix.md"),
    ),
    (
        "policy-keys.md",
        include_str!("../../../docs/spec/rules/policy-keys.md"),
    ),
];

/// The column lists the model implements, one per registered table: (table, file, kind, row prefix, columns). The
/// registry of [RULES/README] §7 must equal this list in both directions ([RULES/README] §7: "any difference in either
/// direction is a parse error").
pub const IMPLEMENTED: [(&str, &str, &str, &str, &str); 115] = [
    (
        "column-types",
        "README.md",
        "meta",
        "CT",
        "row:id, type:token, note:text",
    ),
    (
        "registry",
        "README.md",
        "meta",
        "RG",
        "row:id, table:token, file:token, kind:enum(vocabulary/decision/procedure/map/meta), row_prefix:token, columns:tokens, note:text",
    ),
    (
        "signatures",
        "SIGNED.md",
        "meta",
        "SG",
        "row:id, file:token, blake3:token, signed_on:token, note:text",
    ),
    (
        "merge-classes",
        "merge-table.md",
        "vocabulary",
        "MC",
        "row:id, class:token, key_class:tokens, rules_file:enum(merge-table/link-merge-rules/none), basis:enum, source:cite, note:text",
    ),
    (
        "cases",
        "merge-table.md",
        "vocabulary",
        "CS",
        "row:id, case:token, classes:tokens, definition:text",
    ),
    (
        "results",
        "merge-table.md",
        "vocabulary",
        "RS",
        "row:id, result:token, definition:text",
    ),
    (
        "merge-rules",
        "merge-table.md",
        "decision",
        "MR",
        "row:id, class:token, case:token, result:token, conflict:token, disposition:enum, basis:enum, source:cite, note:text",
    ),
    (
        "status-lattice",
        "merge-table.md",
        "decision",
        "SL",
        "row:id, kind:token, status:token, side:enum(yes/no), covers:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "existence-policy",
        "merge-table.md",
        "decision",
        "EP",
        "row:id, kind:token, uid_derivation:enum(random/file-key/root-key/anchor-key), policy:enum(delete-wins/resurrect/none), basis:enum, source:cite, note:text",
    ),
    (
        "auto-policy",
        "merge-table.md",
        "decision",
        "AP",
        "row:id, value:token, applies_to:token, effect:token, basis:enum, source:cite, note:text",
    ),
    (
        "field-class",
        "merge-table.md",
        "decision",
        "FC",
        "row:id, kind:token, field:token, type:token, class:token, basis:enum, source:cite, note:text",
    ),
    (
        "edge-class",
        "merge-table.md",
        "decision",
        "EC",
        "row:id, edge:token, edge_class:enum(structural/historical), class:token, props:token, constraint:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "class-map",
        "merge-table.md",
        "map",
        "CM",
        "row:id, conflict:token, kind:enum(value/structural/hint), lands:enum(yes/no/log), emitted_by:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "validators",
        "merge-table.md",
        "procedure",
        "VA",
        "row:id, order:int, check:token, emits:token, disposition:enum, basis:enum, source:cite, note:text",
    ),
    (
        "land-or-stage",
        "merge-table.md",
        "decision",
        "LS",
        "row:id, violations:token, conflicts:token, strict:enum(yes/no/any), outcome:token, basis:enum, source:cite, note:text",
    ),
    (
        "procedure",
        "merge-table.md",
        "procedure",
        "PR",
        "row:id, step:token, action:token, basis:enum, source:cite, note:text",
    ),
    (
        "virtual-base",
        "merge-table.md",
        "procedure",
        "VB",
        "row:id, condition:token, outcome:token, basis:enum, source:cite, note:text",
    ),
    (
        "derived-merges",
        "merge-table.md",
        "procedure",
        "DM",
        "row:id, operation:token, dst:token, src:token, base:token, special:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "runtime-effects",
        "merge-table.md",
        "procedure",
        "RE",
        "row:id, class:token, effect:token, basis:enum, source:cite, note:text",
    ),
    (
        "hints",
        "merge-table.md",
        "procedure",
        "HT",
        "row:id, hint:token, trigger:token, basis:enum, source:cite, note:text",
    ),
    (
        "source-map",
        "merge-table.md",
        "map",
        "SM",
        "row:id, source_row:token, realized_by:tokens, note:text",
    ),
    (
        "link-cases",
        "link-merge-rules.md",
        "vocabulary",
        "LC",
        "row:id, case:token, classes:tokens, definition:text",
    ),
    (
        "link-results",
        "link-merge-rules.md",
        "vocabulary",
        "LR",
        "row:id, result:token, definition:text",
    ),
    (
        "link-merge-rules",
        "link-merge-rules.md",
        "decision",
        "LM",
        "row:id, class:token, case:token, result:token, conflict:token, disposition:enum, basis:enum, source:cite, note:text",
    ),
    (
        "path-claim",
        "link-merge-rules.md",
        "procedure",
        "PC",
        "row:id, aspect:token, rule:token, basis:enum, source:cite, note:text",
    ),
    (
        "compose-steps",
        "link-merge-rules.md",
        "procedure",
        "CP",
        "row:id, step:int, action:token, basis:enum, source:cite, note:text",
    ),
    (
        "rekey-steps",
        "link-merge-rules.md",
        "procedure",
        "RK",
        "row:id, step:int, action:token, basis:enum, source:cite, note:text",
    ),
    (
        "link-resolution",
        "link-merge-rules.md",
        "procedure",
        "LV",
        "row:id, conflict:token, resolved_by:token, condition:token, provenance:token, basis:enum, source:cite, note:text",
    ),
    (
        "link-history",
        "link-merge-rules.md",
        "procedure",
        "LH",
        "row:id, operation:tokens, effect:token, basis:enum, source:cite, note:text",
    ),
    (
        "link-source-map",
        "link-merge-rules.md",
        "map",
        "LX",
        "row:id, source_row:token, realized_by:tokens, note:text",
    ),
    (
        "pack-terms",
        "pack-classes.md",
        "vocabulary",
        "PT",
        "row:id, term:token, sort:enum(input/view/set/pred/fn/order), basis:enum, source:cite, definition:text",
    ),
    (
        "pack-kinds",
        "pack-classes.md",
        "decision",
        "PK",
        "row:id, pack:token, trigger:tokens, classes:tokens, budget_key:token, default_bytes:token, basis:enum, source:cite, note:text",
    ),
    (
        "pack-budgets",
        "pack-classes.md",
        "decision",
        "PB",
        "row:id, role:token, key:token, default_bytes:int, final_at:token, basis:enum, source:cite, note:text",
    ),
    (
        "pack-ceilings",
        "pack-classes.md",
        "decision",
        "PE",
        "row:id, surface:enum(cli/mcp/file/hook), client:token, key:token, default_bytes:token, max_bytes:token, basis:enum, source:cite, note:text",
    ),
    (
        "pack-bytes",
        "pack-classes.md",
        "procedure",
        "PY",
        "row:id, rule:token, basis:enum, source:cite, definition:text",
    ),
    (
        "pack-classes",
        "pack-classes.md",
        "vocabulary",
        "CL",
        "row:id, class:token, rank:int, name:token, named_query:token, basis:enum, source:cite, definition:text",
    ),
    (
        "pack-quotas",
        "pack-classes.md",
        "decision",
        "PQ",
        "row:id, class:token, roles:tokens, key:token, default_pct:int, basis:enum, source:cite, note:text",
    ),
    (
        "pack-floors",
        "pack-classes.md",
        "procedure",
        "PF",
        "row:id, class:token, basis:enum, source:cite, definition:text",
    ),
    (
        "pack-members",
        "pack-classes.md",
        "procedure",
        "PM",
        "row:id, class:token, part:token, roles:tokens, view:enum(B/M/U/feed), kinds:tokens, level:token, basis:enum, source:cite, definition:text",
    ),
    (
        "pack-levels",
        "pack-classes.md",
        "procedure",
        "PL",
        "row:id, kind:token, level:enum(ID/L0/L1/L2), content:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "pack-order",
        "pack-classes.md",
        "procedure",
        "PO",
        "row:id, class:token, keys:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "pack-header",
        "pack-classes.md",
        "procedure",
        "PH",
        "row:id, position:int, item:token, when:token, basis:enum, source:cite, definition:text",
    ),
    (
        "pack-render",
        "pack-classes.md",
        "procedure",
        "RN",
        "row:id, rule:token, basis:enum, source:cite, definition:text",
    ),
    (
        "pack-fill",
        "pack-classes.md",
        "procedure",
        "PX",
        "row:id, step:int, action:token, basis:enum, source:cite, definition:text",
    ),
    (
        "hook-pack",
        "pack-classes.md",
        "procedure",
        "HP",
        "row:id, position:int, item:token, level:token, basis:enum, source:cite, definition:text",
    ),
    (
        "brief-classes",
        "pack-classes.md",
        "procedure",
        "BR",
        "row:id, class:token, rank:int, named_query:token, kinds:tokens, level:token, basis:enum, source:cite, definition:text",
    ),
    (
        "delta-rules",
        "pack-classes.md",
        "procedure",
        "DL",
        "row:id, pack:token, rule:token, basis:enum, source:cite, definition:text",
    ),
    (
        "notice-sets",
        "pack-classes.md",
        "procedure",
        "NS",
        "row:id, set:token, code:int, view:enum(B/M), kinds:tokens, basis:enum, source:cite, definition:text",
    ),
    (
        "notice-rules",
        "pack-classes.md",
        "procedure",
        "NR",
        "row:id, step:int, rule:token, basis:enum, source:cite, definition:text",
    ),
    (
        "notice-digest",
        "pack-classes.md",
        "procedure",
        "ND",
        "row:id, offset:token, width:token, type:token, name:token, basis:enum, source:cite, meaning:text",
    ),
    (
        "notice-entry",
        "pack-classes.md",
        "procedure",
        "NE",
        "row:id, offset:token, width:token, type:token, name:token, basis:enum, source:cite, meaning:text",
    ),
    (
        "notice-modes",
        "pack-classes.md",
        "decision",
        "NM",
        "row:id, mode:token, output:token, cap_bytes:int, basis:enum, source:cite, note:text",
    ),
    (
        "pack-source-map",
        "pack-classes.md",
        "map",
        "PS",
        "row:id, source_row:token, realized_by:tokens, note:text",
    ),
    (
        "role-terms",
        "role-write-policy.md",
        "vocabulary",
        "WT",
        "row:id, term:token, sort:enum(input/scope/set/pred), basis:enum, source:cite, definition:text",
    ),
    (
        "role-rights",
        "role-write-policy.md",
        "procedure",
        "WR",
        "row:id, step:int, rule:token, basis:enum, source:cite, definition:text",
    ),
    (
        "role-rows",
        "role-write-policy.md",
        "vocabulary",
        "WO",
        "row:id, role:token, carried_by:tokens, self_claim:enum(yes/no), mcp_write:enum(yes/no), basis:enum, source:cite, note:text",
    ),
    (
        "role-mint",
        "role-write-policy.md",
        "decision",
        "WM",
        "row:id, form:token, commands:tokens, allowed:tokens, key:token, refusal:token, exit:token, basis:enum, source:cite, note:text",
    ),
    (
        "role-verbs",
        "role-write-policy.md",
        "decision",
        "WV",
        "row:id, verb:token, class:enum(ref/graph/runtime/file-fs/file-link/admin/read/surface), surface:enum(cli/mcp/both), roles:tokens, key:token, refusal:token, exit:token, basis:enum, source:cite, note:text",
    ),
    (
        "role-statements",
        "role-write-policy.md",
        "decision",
        "WX",
        "row:id, statement:token, roles:tokens, surface:enum(cli/mcp/both), key:token, refusal:token, exit:token, basis:enum, source:cite, note:text",
    ),
    (
        "role-create",
        "role-write-policy.md",
        "decision",
        "WC",
        "row:id, role:token, kind:token, constraint:tokens, required:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "role-values",
        "role-write-policy.md",
        "decision",
        "WA",
        "row:id, field:token, value:tokens, roles:tokens, requires:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "role-fields",
        "role-write-policy.md",
        "decision",
        "WF",
        "row:id, role:token, kind:token, scope:token, fields:tokens, key:token, basis:enum, source:cite, note:text",
    ),
    (
        "role-status",
        "role-write-policy.md",
        "decision",
        "WS",
        "row:id, role:token, kind:token, scope:token, from:tokens, to:token, via:token, basis:enum, source:cite, note:text",
    ),
    (
        "role-edges",
        "role-write-policy.md",
        "decision",
        "WE",
        "row:id, role:token, edge:token, ops:tokens, src_scope:token, dst_scope:token, basis:enum, source:cite, note:text",
    ),
    (
        "role-reads",
        "role-write-policy.md",
        "decision",
        "WQ",
        "row:id, subject:token, what:token, allowed:enum(yes/no), key:token, refusal:token, exit:token, basis:enum, source:cite, note:text",
    ),
    (
        "role-hooks",
        "role-write-policy.md",
        "procedure",
        "WH",
        "row:id, hook:token, writes:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "role-refusals",
        "role-write-policy.md",
        "vocabulary",
        "WZ",
        "row:id, situation:token, code:token, name:token, exit:token, text_owner:token, basis:enum, source:cite, note:text",
    ),
    (
        "role-source-map",
        "role-write-policy.md",
        "map",
        "WY",
        "row:id, source_row:token, realized_by:tokens, note:text",
    ),
    (
        "view-kinds",
        "state-definition.md",
        "decision",
        "VK",
        "row:id, ref_kind:token, holds_count:enum(yes/no), tip_reads:enum(yes/no), basis:enum, source:cite, note:text",
    ),
    (
        "hold-values",
        "state-definition.md",
        "decision",
        "HV",
        "row:id, kind:token, state:token, hold:token, basis:enum, source:cite, note:text",
    ),
    (
        "origin-rules",
        "state-definition.md",
        "decision",
        "OR",
        "row:id, parents:int, condition:token, origin:token, basis:enum, source:cite, note:text",
    ),
    (
        "predicates",
        "state-definition.md",
        "procedure",
        "PD",
        "row:id, predicate:token, clause:token, basis:enum, source:cite, note:text",
    ),
    (
        "validity",
        "state-definition.md",
        "decision",
        "VD",
        "row:id, predicate:token, valid_at:enum(any-view/tip-only), past_view:token, basis:enum, source:cite, note:text",
    ),
    (
        "blocker-terms",
        "state-definition.md",
        "decision",
        "BT",
        "row:id, edge:token, source_state:token, counts_in:token, weight:int, basis:enum, source:cite, note:text",
    ),
    (
        "lease-live",
        "state-definition.md",
        "decision",
        "LL",
        "row:id, scope:enum(run/ttl), anchor:token, boot:token, slot:token, deadline:token, live:token, basis:enum, source:cite, note:text",
    ),
    (
        "lease-ends",
        "state-definition.md",
        "procedure",
        "LE",
        "row:id, event:token, effect:token, basis:enum, source:cite, note:text",
    ),
    (
        "lease-effects",
        "state-definition.md",
        "procedure",
        "LF",
        "row:id, consumer:token, rule:token, basis:enum, source:cite, note:text",
    ),
    (
        "marker-fields",
        "state-definition.md",
        "vocabulary",
        "MF",
        "row:id, field:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "marker-events",
        "state-definition.md",
        "procedure",
        "ME",
        "row:id, event:token, condition:token, record:token, basis:enum, source:cite, note:text",
    ),
    (
        "absorption",
        "state-definition.md",
        "decision",
        "AB",
        "row:id, marker_state:token, test:token, basis:enum, source:cite, note:text",
    ),
    (
        "vector-rules",
        "state-definition.md",
        "procedure",
        "VR",
        "row:id, event:token, rule:token, basis:enum, source:cite, note:text",
    ),
    (
        "door-coverage",
        "state-definition.md",
        "map",
        "DC",
        "row:id, door:token, events:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "scenarios",
        "state-definition.md",
        "decision",
        "SN",
        "row:id, scenario:token, step:int, ref:token, action:token, basis:enum, source:cite, note:text",
    ),
    (
        "scenario-expect",
        "state-definition.md",
        "decision",
        "SX",
        "row:id, scenario:token, step:int, check_ref:token, excluded:enum(yes/no), basis:enum, source:cite, note:text",
    ),
    (
        "status-fields",
        "status-machines.md",
        "decision",
        "SF",
        "row:id, kind:token, field:token, stored:enum(yes/no), guarded:enum(yes/no), basis:enum, source:cite, note:text",
    ),
    (
        "statuses",
        "status-machines.md",
        "decision",
        "ST",
        "row:id, kind:token, status:token, initial:enum(yes/no), done:enum(yes/no/derived/absent), lattice:token, basis:enum, source:cite, note:text",
    ),
    (
        "doors",
        "status-machines.md",
        "vocabulary",
        "DR",
        "row:id, door:token, requires:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "transitions",
        "status-machines.md",
        "decision",
        "TR",
        "row:id, kind:token, from:token, to:token, door:token, move:enum(up/down/to-side/from-side), basis:enum, source:cite, note:text",
    ),
    (
        "guards",
        "status-machines.md",
        "vocabulary",
        "GD",
        "row:id, guard:token, refusal:token, exit:token, basis:enum, source:cite, definition:text",
    ),
    (
        "transition-guards",
        "status-machines.md",
        "decision",
        "TG",
        "row:id, kind:token, from:token, to:token, guard:token, basis:enum, source:cite, note:text",
    ),
    (
        "door-roles",
        "status-machines.md",
        "map",
        "DG",
        "row:id, door:token, realized_by:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "branch-mask",
        "status-machines.md",
        "decision",
        "BM",
        "row:id, view:token, status_writes:enum(yes/no), refusal:token, exit:token, basis:enum, source:cite, note:text",
    ),
    (
        "derived-effects",
        "status-machines.md",
        "procedure",
        "DE",
        "row:id, kind:token, from:token, to:token, predicate:token, subject:token, basis:enum, source:cite, note:text",
    ),
    (
        "complete-outcomes",
        "status-machines.md",
        "decision",
        "CO",
        "row:id, outcome:token, status:token, lease:token, hold:token, basis:enum, source:cite, note:text",
    ),
    (
        "general-rules",
        "status-machines.md",
        "procedure",
        "GR",
        "row:id, rule:token, applies_to:token, refusal:token, exit:token, basis:enum, source:cite, note:text",
    ),
    (
        "delete-options",
        "delete-policy-matrix.md",
        "vocabulary",
        "DO",
        "row:id, option:token, basis:enum, source:cite, definition:text",
    ),
    (
        "edge-conditions",
        "delete-policy-matrix.md",
        "vocabulary",
        "CD",
        "row:id, condition:token, basis:enum, source:cite, definition:text",
    ),
    (
        "edge-actions",
        "delete-policy-matrix.md",
        "vocabulary",
        "EA",
        "row:id, action:token, basis:enum, source:cite, definition:text",
    ),
    (
        "edge-effects",
        "delete-policy-matrix.md",
        "vocabulary",
        "EF",
        "row:id, effect:token, basis:enum, source:cite, definition:text",
    ),
    (
        "delete-preconditions",
        "delete-policy-matrix.md",
        "procedure",
        "DP",
        "row:id, check:token, refusal:token, exit:token, basis:enum, source:cite, note:text",
    ),
    (
        "edge-policy",
        "delete-policy-matrix.md",
        "decision",
        "EG",
        "row:id, edge:token, end:enum(dst-deleted/src-deleted), option:token, policy:token, condition:token, action:token, effect:token, basis:enum, source:cite, note:text",
    ),
    (
        "delete-steps",
        "delete-policy-matrix.md",
        "procedure",
        "DS",
        "row:id, step:token, action:token, basis:enum, source:cite, note:text",
    ),
    (
        "flagged-edges",
        "delete-policy-matrix.md",
        "procedure",
        "FL",
        "row:id, rule:token, basis:enum, source:cite, note:text",
    ),
    (
        "tombstone",
        "delete-policy-matrix.md",
        "decision",
        "TB",
        "row:id, item:token, kept:enum(yes/no), basis:enum, source:cite, note:text",
    ),
    (
        "undelete",
        "delete-policy-matrix.md",
        "procedure",
        "UD",
        "row:id, item:token, effect:token, basis:enum, source:cite, note:text",
    ),
    (
        "cross-branch",
        "delete-policy-matrix.md",
        "map",
        "XB",
        "row:id, case:token, realized_by:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "n40-nodes",
        "delete-policy-matrix.md",
        "decision",
        "NN",
        "row:id, node:token, kind:token, status:token, parent:token, note:text",
    ),
    (
        "n40-edges",
        "delete-policy-matrix.md",
        "decision",
        "NG",
        "row:id, src:token, kind:token, dst:token, props:token, note:text",
    ),
    (
        "n40-cases",
        "delete-policy-matrix.md",
        "decision",
        "NC",
        "row:id, case:token, ref:token, after:token, action:token, basis:enum, source:cite, note:text",
    ),
    (
        "n40-properties",
        "delete-policy-matrix.md",
        "vocabulary",
        "NP",
        "row:id, property:token, definition:text",
    ),
    (
        "n40-expect",
        "delete-policy-matrix.md",
        "decision",
        "NX",
        "row:id, case:token, ref:token, subject:token, property:token, value:token, basis:enum, source:cite, note:text",
    ),
    (
        "key-checkers",
        "policy-keys.md",
        "vocabulary",
        "KC",
        "row:id, checker:token, vis:tokens, basis:enum, source:cite, definition:text",
    ),
    (
        "key-functions",
        "policy-keys.md",
        "vocabulary",
        "KF",
        "row:id, function:token, wp:token, basis:enum, source:cite, definition:text",
    ),
    (
        "policy-keys",
        "policy-keys.md",
        "decision",
        "KY",
        "row:id, key:token, instance:token, vis:enum(V/I/Rs/B/O/X), function:token, values:tokens, checker:tokens, basis:enum, source:cite, note:text",
    ),
    (
        "policy-rows",
        "policy-keys.md",
        "decision",
        "PV",
        "row:id, row_name:token, instance:token, function:token, values:tokens, basis:enum, source:cite, note:text",
    ),
];

/// The kind of a registered table ([RULES/README] §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableKind {
    /// Defines tokens other tables use.
    Vocabulary,
    /// Rows evaluated as data.
    Decision,
    /// Rows implemented as tagged functions.
    Procedure,
    /// Cross-references, checked for dangling references.
    Map,
    /// The README's own tables and `SIGNED.md`.
    Meta,
}

impl TableKind {
    fn parse(s: &str) -> Option<TableKind> {
        Some(match s {
            "vocabulary" => TableKind::Vocabulary,
            "decision" => TableKind::Decision,
            "procedure" => TableKind::Procedure,
            "map" => TableKind::Map,
            "meta" => TableKind::Meta,
            _ => return None,
        })
    }
}

/// One data row of a table.
#[derive(Clone, Debug)]
pub struct Row {
    /// The row id.
    pub id: String,
    /// The 1-based line in its file.
    pub line: usize,
    /// The typed cells by column name, in column order.
    pub cells: Vec<(String, Cell)>,
}

impl Row {
    /// The cell of a column.
    pub fn get(&self, col: &str) -> &Cell {
        self.cells
            .iter()
            .find(|(n, _)| n == col)
            .map(|(_, c)| c)
            .unwrap_or_else(|| panic!("row {} has no column {col}", self.id))
    }

    /// The single token of a column (backticks stripped).
    pub fn tok(&self, col: &str) -> &str {
        self.get(col).tok()
    }

    /// The token list of a column.
    pub fn toks(&self, col: &str) -> Vec<&str> {
        self.get(col).toks()
    }

    /// The `int` value of a column.
    pub fn int(&self, col: &str) -> u64 {
        self.get(col).int()
    }

    /// The `basis` of the row, where the table has that column.
    pub fn basis(&self) -> Option<&str> {
        self.cells
            .iter()
            .find(|(n, _)| n == "basis")
            .map(|(_, c)| c.tok())
    }
}

/// One parsed, typed table.
#[derive(Clone, Debug)]
pub struct Table {
    /// The table id.
    pub id: String,
    /// The file that holds it.
    pub file: String,
    /// Its kind.
    pub kind: TableKind,
    /// Its row-id prefix.
    pub prefix: String,
    /// Its columns.
    pub columns: Vec<Column>,
    /// Its rows in file order (the evaluation order of a decision table).
    pub rows: Vec<Row>,
}

impl Table {
    /// The row with this id.
    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }
}

/// Every rule file, parsed and checked.
#[derive(Debug)]
pub struct Rules {
    tables: BTreeMap<String, Table>,
    rows: BTreeMap<String, String>,
    open_points: BTreeMap<String, BTreeSet<u32>>,
}

impl Rules {
    /// The table with this id; a table the registry does not list is a model bug.
    pub fn table(&self, id: &str) -> &Table {
        self.tables
            .get(id)
            .unwrap_or_else(|| panic!("no rule table {id}"))
    }

    /// The row with this id in any file, with its table.
    pub fn row(&self, id: &str) -> Option<(&Table, &Row)> {
        let t = self.tables.get(self.rows.get(id)?)?;
        Some((t, t.row(id)?))
    }

    /// Every table, by id.
    pub fn tables(&self) -> impl Iterator<Item = &Table> {
        self.tables.values()
    }

    /// The open-point numbers of a file.
    pub fn open_points(&self, file: &str) -> Option<&BTreeSet<u32>> {
        self.open_points.get(file)
    }
}

/// The rule tables of this process: parsed at the first call, before any model function reads a rule
/// ([RULES/README] §8 "Parsing"). A violation of the contract panics with the file, line, table and column.
pub fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| match load(&FILES) {
        Ok(r) => r,
        Err(e) => panic!("rule tables: {e}"),
    })
}

fn perr(
    file: &str,
    line: usize,
    table: Option<&str>,
    column: Option<&str>,
    what: String,
) -> ParseError {
    ParseError {
        file: file.to_string(),
        line,
        table: table.map(str::to_string),
        column: column.map(str::to_string),
        what,
    }
}

/// One registry row, as the loader reads it.
struct Registered {
    file: String,
    kind: TableKind,
    prefix: String,
    columns: Vec<Column>,
}

/// The registry's own columns, compiled in: the registry cannot describe itself before it is read.
fn registry_columns() -> Vec<Column> {
    IMPLEMENTED
        .iter()
        .find(|t| t.0 == "registry")
        .map(|t| t.4)
        .into_iter()
        .flat_map(|c| c.split(", "))
        .map(|c| parse::parse_column(c).expect("the compiled registry columns parse"))
        .collect()
}

fn type_rows(
    file: &str,
    raw: &parse::RawTable,
    columns: &[Column],
    prefix: &str,
) -> Result<Vec<Row>, ParseError> {
    let header: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
    if raw.header != header {
        return Err(perr(
            file,
            raw.marker_line + 1,
            Some(&raw.id),
            None,
            format!(
                "header {:?} differs from the registered columns {:?}",
                raw.header, header
            ),
        ));
    }
    let mut rows = Vec::new();
    for (line, cells) in &raw.rows {
        let mut typed = Vec::new();
        for (col, raw_cell) in columns.iter().zip(cells) {
            let cell = parse::type_cell(raw_cell, col, prefix)
                .map_err(|w| perr(file, *line, Some(&raw.id), Some(&col.name), w))?;
            typed.push((col.name.clone(), cell));
        }
        let id = typed[0].1.tok().to_string();
        rows.push(Row {
            id,
            line: *line,
            cells: typed,
        });
    }
    Ok(rows)
}

/// Checks a registered column list against the table grammar ([RULES/README] §4): the first column is `row:id`, and
/// exactly one `text` column exists and is the last.
fn check_shape(table: &str, columns: &[Column]) -> Result<(), String> {
    if columns.first().map(|c| (c.name.as_str(), &c.ty)) != Some(("row", &ColType::Id)) {
        return Err(format!("{table}: the first column is not row:id"));
    }
    let texts: Vec<usize> = columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.ty == ColType::Text)
        .map(|(i, _)| i)
        .collect();
    if texts != [columns.len() - 1] {
        return Err(format!(
            "{table}: exactly one text column, the last, is required"
        ));
    }
    Ok(())
}

/// Parses and checks a set of rule files ([RULES/README] §2–§8). `files` must hold `README.md`, whose registry drives
/// every other file.
pub fn load(files: &[(&str, &str)]) -> Result<Rules, ParseError> {
    let mut raw: BTreeMap<String, parse::RawFile> = BTreeMap::new();
    for (name, text) in files {
        raw.insert(name.to_string(), parse::scan(name, text)?);
    }
    let readme = raw
        .get("README.md")
        .ok_or_else(|| perr("README.md", 0, None, None, "README.md is missing".into()))?;
    let reg_raw = readme
        .tables
        .iter()
        .find(|t| t.id == "registry")
        .ok_or_else(|| {
            perr(
                "README.md",
                0,
                Some("registry"),
                None,
                "no registry table".into(),
            )
        })?;
    let reg_rows = type_rows("README.md", reg_raw, &registry_columns(), "RG")?;
    let mut registry: BTreeMap<String, Registered> = BTreeMap::new();
    for r in &reg_rows {
        let table = r.tok("table").to_string();
        let kind = TableKind::parse(r.tok("kind")).ok_or_else(|| {
            perr(
                "README.md",
                r.line,
                Some("registry"),
                Some("kind"),
                "unknown kind".into(),
            )
        })?;
        let mut columns = Vec::new();
        for spec in r.toks("columns") {
            columns
                .push(parse::parse_column(spec).map_err(|w| {
                    perr("README.md", r.line, Some("registry"), Some("columns"), w)
                })?);
        }
        check_shape(&table, &columns)
            .map_err(|w| perr("README.md", r.line, Some("registry"), Some("columns"), w))?;
        let prefix = r.tok("row_prefix").to_string();
        if prefix.len() != 2 || !prefix.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(perr(
                "README.md",
                r.line,
                Some("registry"),
                Some("row_prefix"),
                format!("bad prefix {prefix}"),
            ));
        }
        if registry
            .insert(
                table.clone(),
                Registered {
                    file: r.tok("file").to_string(),
                    kind,
                    prefix,
                    columns,
                },
            )
            .is_some()
        {
            return Err(perr(
                "README.md",
                r.line,
                Some("registry"),
                Some("table"),
                format!("{table} is registered twice"),
            ));
        }
    }
    // The registry equals the model's compiled-in column lists, in both directions (§7).
    for (table, file, kind, prefix, cols) in IMPLEMENTED {
        let r = registry.get(table).ok_or_else(|| {
            perr(
                "README.md",
                reg_raw.marker_line,
                Some("registry"),
                None,
                format!("the model implements {table}, which the registry does not list"),
            )
        })?;
        let want: Vec<Column> = cols
            .split(", ")
            .map(|c| parse::parse_column(c).expect("compiled column lists parse"))
            .collect();
        if r.file != file
            || TableKind::parse(kind) != Some(r.kind)
            || r.prefix != prefix
            || r.columns != want
        {
            return Err(perr(
                "README.md",
                reg_raw.marker_line,
                Some("registry"),
                None,
                format!("the registry row of {table} differs from the model's column list"),
            ));
        }
    }
    for table in registry.keys() {
        if !IMPLEMENTED.iter().any(|t| t.0 == table) {
            return Err(perr(
                "README.md",
                reg_raw.marker_line,
                Some("registry"),
                None,
                format!("the registry lists {table}, which the model does not implement"),
            ));
        }
    }
    let prefixes: BTreeSet<&str> = registry.values().map(|r| r.prefix.as_str()).collect();
    if prefixes.len() != registry.len() {
        return Err(perr(
            "README.md",
            reg_raw.marker_line,
            Some("registry"),
            Some("row_prefix"),
            "two tables share a prefix".into(),
        ));
    }
    // Every marker is registered for its file; every registered table of a present file exists (§3).
    let mut tables = BTreeMap::new();
    let mut rows_index = BTreeMap::new();
    let mut open_points = BTreeMap::new();
    for (file, f) in &raw {
        open_points.insert(
            file.clone(),
            f.open_points.iter().copied().collect::<BTreeSet<u32>>(),
        );
        let mut seen = BTreeSet::new();
        for t in &f.tables {
            let r = registry.get(&t.id).ok_or_else(|| {
                perr(
                    file,
                    t.marker_line,
                    Some(&t.id),
                    None,
                    "the table id is not in the registry".into(),
                )
            })?;
            if &r.file != file {
                return Err(perr(
                    file,
                    t.marker_line,
                    Some(&t.id),
                    None,
                    format!("the table is registered for {}", r.file),
                ));
            }
            if !seen.insert(t.id.clone()) {
                return Err(perr(
                    file,
                    t.marker_line,
                    Some(&t.id),
                    None,
                    "the table appears twice".into(),
                ));
            }
            let rows = if t.id == "registry" {
                reg_rows.clone()
            } else {
                type_rows(file, t, &r.columns, &r.prefix)?
            };
            for row in &rows {
                if let Some(other) = rows_index.insert(row.id.clone(), t.id.clone()) {
                    return Err(perr(
                        file,
                        row.line,
                        Some(&t.id),
                        Some("row"),
                        format!("row id {} is also used in {other}", row.id),
                    ));
                }
            }
            tables.insert(
                t.id.clone(),
                Table {
                    id: t.id.clone(),
                    file: file.clone(),
                    kind: r.kind,
                    prefix: r.prefix.clone(),
                    columns: r.columns.clone(),
                    rows,
                },
            );
        }
        for (id, r) in &registry {
            if &r.file == file && !seen.contains(id) {
                return Err(perr(
                    file,
                    0,
                    Some(id),
                    None,
                    "a registered table is missing from its file".into(),
                ));
            }
        }
    }
    for (id, r) in &registry {
        if r.file != "SIGNED.md" && !raw.contains_key(&r.file) {
            return Err(perr(
                &r.file,
                0,
                Some(id),
                None,
                "the registered file is not loaded".into(),
            ));
        }
    }
    let rules = Rules {
        tables,
        rows: rows_index,
        open_points,
    };
    check_references(&rules)?;
    Ok(rules)
}

fn ref_err(t: &Table, r: &Row, col: &str, what: String) -> ParseError {
    perr(&t.file, r.line, Some(&t.id), Some(col), what)
}

/// Whether a token has the shape of a row id (`XX-000`).
fn looks_like_row_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 6
        && b[0].is_ascii_uppercase()
        && b[1].is_ascii_uppercase()
        && b[2] == b'-'
        && b[3..].iter().all(u8::is_ascii_digit)
}

/// The referential checks of [RULES/README] §8 "Referential checks at load", and the vocabulary checks of the decision
/// tables whose tokens a vocabulary table defines.
fn check_references(rules: &Rules) -> Result<(), ParseError> {
    let vocab = |table: &str, col: &str| -> BTreeSet<String> {
        rules.tables.get(table).map_or_else(BTreeSet::new, |t| {
            t.rows.iter().map(|r| r.tok(col).to_string()).collect()
        })
    };
    // Row ids cited in realized_by, emitted_by and events cells exist.
    for t in rules.tables.values() {
        for r in &t.rows {
            for col in ["realized_by", "emitted_by", "events"] {
                if t.columns.iter().any(|c| c.name == col) {
                    for id in r.toks(col) {
                        if id != "-" && looks_like_row_id(id) && rules.row(id).is_none() {
                            return Err(ref_err(t, r, col, format!("row {id} does not exist")));
                        }
                    }
                }
            }
            // Every [OP-n] a cite or text cell names exists in the file's open points.
            let ops = rules.open_points.get(&t.file).cloned().unwrap_or_default();
            for (col, cell) in &r.cells {
                let text = match cell {
                    Cell::Cite(v) => v.join("; "),
                    Cell::Text(s) => s.clone(),
                    _ => continue,
                };
                let mut rest = text.as_str();
                while let Some(p) = rest.find("[OP-") {
                    let tail = &rest[p + 4..];
                    let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
                    if tail[digits.len()..].starts_with(']') && !digits.is_empty() {
                        let n: u32 = digits.parse().expect("digits");
                        if !ops.contains(&n) {
                            return Err(ref_err(
                                t,
                                r,
                                col,
                                format!("[OP-{n}] is not an open point of {}", t.file),
                            ));
                        }
                    }
                    rest = &tail[digits.len()..];
                }
            }
        }
    }
    // Merge classes: every class a field-class or edge-class row uses exists, and its rules live in exactly the file
    // merge-classes names.
    let classes = vocab("merge-classes", "class");
    for (table, col) in [("field-class", "class"), ("edge-class", "class")] {
        let t = rules.table(table);
        for r in &t.rows {
            if !classes.contains(r.tok(col)) {
                return Err(ref_err(
                    t,
                    r,
                    col,
                    format!("merge class {} is not in merge-classes", r.tok(col)),
                ));
            }
        }
    }
    let mt = rules.table("merge-rules");
    let lt = rules.table("link-merge-rules");
    for r in &rules.table("merge-classes").rows {
        let c = r.tok("class");
        let in_mt = mt.rows.iter().any(|x| x.tok("class") == c);
        let in_lt = lt.rows.iter().any(|x| x.tok("class") == c);
        let keyed = r.toks("key_class") != ["none"];
        let ok = match r.tok("rules_file") {
            "merge-table" => !in_lt && in_mt == keyed,
            "link-merge-rules" => in_lt && !in_mt,
            _ => !in_lt && !in_mt,
        };
        if !ok {
            let t = rules.table("merge-classes");
            return Err(ref_err(
                t,
                r,
                "rules_file",
                format!("the rules of class {c} are not where rules_file says"),
            ));
        }
    }
    // Every conflict class a rule row emits appears in class-map; cases and results are defined.
    let conflicts = vocab("class-map", "conflict");
    let cases: BTreeSet<String> = vocab("cases", "case")
        .into_iter()
        .chain(vocab("link-cases", "case"))
        .collect();
    let results: BTreeSet<String> = vocab("results", "result")
        .into_iter()
        .chain(vocab("link-results", "result"))
        .collect();
    for t in [mt, lt] {
        for r in &t.rows {
            let c = r.tok("conflict");
            if c != "-" && !conflicts.contains(c) {
                return Err(ref_err(
                    t,
                    r,
                    "conflict",
                    format!("conflict class {c} is not in class-map"),
                ));
            }
            if !cases.contains(r.tok("case")) {
                return Err(ref_err(
                    t,
                    r,
                    "case",
                    format!("case {} is not defined", r.tok("case")),
                ));
            }
            if !results.contains(r.tok("result")) {
                return Err(ref_err(
                    t,
                    r,
                    "result",
                    format!("result {} is not defined", r.tok("result")),
                ));
            }
        }
    }
    // Status machines: doors and guards are defined; every `statuses` row names its lattice row.
    let doors = vocab("doors", "door");
    let t = rules.table("transitions");
    for r in &t.rows {
        if !doors.contains(r.tok("door")) {
            return Err(ref_err(
                t,
                r,
                "door",
                format!("door {} is not defined", r.tok("door")),
            ));
        }
    }
    let guards = vocab("guards", "guard");
    let t = rules.table("transition-guards");
    for r in &t.rows {
        if !guards.contains(r.tok("guard")) {
            return Err(ref_err(
                t,
                r,
                "guard",
                format!("guard {} is not defined", r.tok("guard")),
            ));
        }
    }
    let t = rules.table("door-roles");
    for r in &t.rows {
        if !doors.contains(r.tok("door")) {
            return Err(ref_err(
                t,
                r,
                "door",
                format!("door {} is not defined", r.tok("door")),
            ));
        }
    }
    let sl = rules.table("status-lattice");
    let t = rules.table("statuses");
    for r in &t.rows {
        let l = sl.row(r.tok("lattice"));
        if l.is_none_or(|l| l.tok("kind") != r.tok("kind") || l.tok("status") != r.tok("status")) {
            return Err(ref_err(
                t,
                r,
                "lattice",
                format!(
                    "{} is not the lattice row of {}.{}",
                    r.tok("lattice"),
                    r.tok("kind"),
                    r.tok("status")
                ),
            ));
        }
    }
    // The delete-policy matrix uses only its vocabularies.
    let t = rules.table("edge-policy");
    for (col, voc, wild) in [
        ("option", "delete-options", true),
        ("condition", "edge-conditions", false),
        ("action", "edge-actions", false),
        ("effect", "edge-effects", false),
    ] {
        let v = vocab(
            voc,
            if voc == "delete-options" {
                "option"
            } else {
                col
            },
        );
        for r in &t.rows {
            let x = r.tok(col);
            if !(v.contains(x) || (wild && x == "*")) {
                return Err(ref_err(t, r, col, format!("{x} is not defined by {voc}")));
            }
        }
    }
    let props = vocab("n40-properties", "property");
    let t = rules.table("n40-expect");
    for r in &t.rows {
        if !props.contains(r.tok("property")) {
            return Err(ref_err(
                t,
                r,
                "property",
                format!("property {} is not defined", r.tok("property")),
            ));
        }
    }
    // The policy keys use the functions and checkers their vocabularies define.
    let functions = vocab("key-functions", "function");
    let checkers = vocab("key-checkers", "checker");
    for (table, col, voc, name) in [
        ("policy-keys", "function", &functions, "key-functions"),
        ("policy-keys", "checker", &checkers, "key-checkers"),
        ("policy-rows", "function", &functions, "key-functions"),
    ] {
        let t = rules.table(table);
        for r in &t.rows {
            let names = if col == "checker" {
                r.toks(col)
            } else {
                vec![r.tok(col)]
            };
            for n in names {
                if !voc.contains(n) {
                    return Err(ref_err(t, r, col, format!("{n} is not defined by {name}")));
                }
            }
        }
    }
    // Role cells name roles of role-rows or the wildcards of [RULES/role-write-policy] §2.
    let roles: BTreeSet<String> = vocab("role-rows", "role");
    let wild = ["*", "leased", "holder", "-"];
    for (table, col, list) in [
        ("role-mint", "allowed", true),
        ("role-verbs", "roles", true),
        ("role-statements", "roles", true),
        ("role-values", "roles", true),
        ("role-create", "role", false),
        ("role-fields", "role", false),
        ("role-status", "role", false),
        ("role-edges", "role", false),
    ] {
        let t = rules.table(table);
        for r in &t.rows {
            let names = if list { r.toks(col) } else { vec![r.tok(col)] };
            for n in names {
                if !(roles.contains(n) || wild.contains(&n)) {
                    return Err(ref_err(t, r, col, format!("role {n} is not in role-rows")));
                }
            }
        }
    }
    Ok(())
}

/// The owner's signatures read from `docs/spec/rules/SIGNED.md` at run time ([RULES/README] §6): (file, BLAKE3 hex),
/// or `None` while the owner has not written the file.
pub fn signed_rows() -> Option<Vec<(String, String)>> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/rules/SIGNED.md");
    let text = std::fs::read_to_string(path).ok()?;
    let f = match parse::scan("SIGNED.md", &text) {
        Ok(f) => f,
        Err(e) => panic!("rule tables: {e}"),
    };
    let cols: Vec<Column> = IMPLEMENTED
        .iter()
        .find(|t| t.0 == "signatures")
        .map(|t| t.4)
        .into_iter()
        .flat_map(|c| c.split(", "))
        .map(|c| parse::parse_column(c).expect("compiled columns parse"))
        .collect();
    let t = f
        .tables
        .iter()
        .find(|t| t.id == "signatures")
        .unwrap_or_else(|| panic!("rule tables: SIGNED.md has no signatures table"));
    let rows =
        type_rows("SIGNED.md", t, &cols, "SG").unwrap_or_else(|e| panic!("rule tables: {e}"));
    Some(
        rows.iter()
            .map(|r| (r.tok("file").to_string(), r.tok("blake3").to_string()))
            .collect(),
    )
}

/// The BLAKE3-256 digest of a rule file's bytes as 64 lower-case hex digits ([RULES/README] §6 "Digest").
pub fn digest(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_file_parses_and_checks() {
        let r = rules();
        assert_eq!(
            r.tables().count(),
            114,
            "every registered table but SIGNED.md's"
        );
        assert!(r.row("MR-012").is_some());
        assert_eq!(r.table("transitions").kind, TableKind::Decision);
        assert!(
            r.open_points("merge-table.md")
                .is_some_and(|o| o.contains(&1))
        );
    }

    #[test]
    fn rules_print_digests() {
        for (name, text) in FILES {
            println!("{name} {}", digest(text));
        }
    }

    #[test]
    fn rules_signed() {
        let tier = std::env::var("MOIRAI_TEST_TIER").unwrap_or_default();
        let signed = signed_rows().unwrap_or_default();
        for (name, text) in FILES {
            match signed.iter().find(|(f, _)| f == name) {
                Some((_, want)) => {
                    let got = digest(text);
                    assert_eq!(
                        &got, want,
                        "{name}: the signed digest {want} differs from the parsed bytes' {got}"
                    );
                }
                None => assert_ne!(
                    tier, "exit",
                    "{name} is unsigned; tier exit requires every rule file signed"
                ),
            }
        }
    }

    fn with_replaced(file: &str, from: &str, to: &str) -> Vec<(&'static str, String)> {
        FILES
            .iter()
            .map(|(n, t)| {
                let t = if *n == file {
                    t.replacen(from, to, 1)
                } else {
                    t.to_string()
                };
                (*n, t)
            })
            .collect()
    }

    fn load_owned(files: &[(&'static str, String)]) -> Result<Rules, ParseError> {
        let v: Vec<(&str, &str)> = files.iter().map(|(n, t)| (*n, t.as_str())).collect();
        load(&v)
    }

    #[test]
    fn seeded_violations_are_refused() {
        // A duplicate row id across files.
        let e = load_owned(&with_replaced(
            "status-machines.md",
            "| TR-002 |",
            "| TR-001 |",
        ))
        .unwrap_err();
        assert!(e.what.contains("TR-001"), "{e}");
        // An unregistered table id.
        let e = load_owned(&with_replaced(
            "status-machines.md",
            "<!-- table: doors -->",
            "<!-- table: doorz -->",
        ))
        .unwrap_err();
        assert!(e.what.contains("not in the registry"), "{e}");
        // A reordered header.
        let e = load_owned(&with_replaced(
            "status-machines.md",
            "| row | door | requires |",
            "| row | requires | door |",
        ))
        .unwrap_err();
        assert!(e.what.contains("header"), "{e}");
        // A value outside an enumeration.
        let e = load_owned(&with_replaced(
            "status-machines.md",
            "| TR-001 | task | `open` | `in_progress` | set-status | up |",
            "| TR-001 | task | `open` | `in_progress` | set-status | sideways |",
        ))
        .unwrap_err();
        assert_eq!(e.column.as_deref(), Some("move"), "{e}");
        // A dangling realized_by reference.
        let e = load_owned(&with_replaced(
            "status-machines.md",
            "| DG-005 | reopen | WX-006 |",
            "| DG-005 | reopen | WX-996 |",
        ))
        .unwrap_err();
        assert!(e.what.contains("WX-996"), "{e}");
        // An undefined door.
        let e = load_owned(&with_replaced(
            "status-machines.md",
            "| TR-015 | task | `done` | `open` | reopen |",
            "| TR-015 | task | `done` | `open` | reopenx |",
        ))
        .unwrap_err();
        assert!(e.what.contains("door"), "{e}");
        // A carriage return.
        let e = load_owned(&with_replaced("merge-table.md", "\n", "\r\n")).unwrap_err();
        assert!(e.what.contains("carriage return"), "{e}");
        // An open point that does not exist.
        let e = load_owned(&with_replaced("status-machines.md", "[OP-10]", "[OP-99]")).unwrap_err();
        assert!(e.what.contains("OP-99"), "{e}");
        // A registered table removed from its file.
        let e = load_owned(&with_replaced(
            "status-machines.md",
            "<!-- table: guards -->\n",
            "",
        ))
        .unwrap_err();
        assert!(
            e.what.contains("header") || e.what.contains("missing"),
            "{e}"
        );
    }
}
