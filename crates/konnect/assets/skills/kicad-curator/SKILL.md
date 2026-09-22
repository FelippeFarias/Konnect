---
name: kicad-curator
description: |
  Lesson triage for Konnect flow jobs: the three places a lesson can go, the
  candidate entry format, the playbook note, and the promotion rules the
  orchestrating session applies at the learn phase. Triggers on: "close out this
  job", "record lessons", "what did we learn", "learn phase", "promote lessons".
argument-hint: "[job_id of a job at its learn phase]"
---

# Konnect Curator — One Lesson, One Destination

A lesson written where the next run does not read it is lost; a lesson
written in two places drifts. This skill is the one home of the triage rule.
Every agent uses it the moment a lesson happens — an agent's own mistake is
recorded when it is seen, because the detail is gone by the end of the job.
The curator agent, or the session itself under a client without bundled
agents, uses it at `learn` to distill the whole job.

---

## The three destinations

| Tier | Path | Written by | Read by |
|---|---|---|---|
| Global role | `~/.konnect/agents/<role>/MEMORY.md`, at most 60 lines | The orchestrating session's own editor, only when promoting a `role` candidate marked `Promote: yes` at `learn` | The session, pasted verbatim into each brief's Context |
| Project role | `memory/<role>.md` under `.konnect/flow/` | `flow_log` with `kind` `lesson` and `scope` `project` | The session, into briefs; agents, with `flow_status(project_dir, read)` |
| Candidates queue | `records/lessons-candidates.md` under `.konnect/flow/` | `flow_log` with `kind` `lesson` and `scope` `role` or `technology` | The curator; the user at `learn`; `technology` entries wait for the brain |

No flow tool writes under the user's home directory. The global tier is the
session's to edit, and nobody else's.

## The triage rule

Ask in this order. The first yes decides, and the lesson goes to exactly one
destination:

1. **About how a role works** — true for that role on any board → `scope`
   `role`, into the candidates queue, promotable to the role's global memory.
2. **About a technology or a part**, and true of it anywhere (ESP32,
   TPIC6B595, JLCPCB) → `scope` `technology`, into the candidates queue, where
   it stays.
3. **Otherwise** — bound to this project → `scope` `project`, into
   `memory/<role>.md`.

When the answer is unclear, take `project`. A project note that proves
general can be promoted later; a false general rule misleads every board after
it.

`role` is the role the lesson is for, one of: `requirements`, `architecture`,
`sourcing`, `schematic`, `library`, `layout`, `review`, `manufacture`,
`photo-intake`, `design-reconstruction`, `curator`, `orchestrator`.

## Recording a lesson

```
flow_log(project_dir, job_id, kind, message, role, scope)
```

`kind` is `lesson`; `role` and `scope` are both required. `message` is the
candidate entry below. `flow_log` is accepted on a closed job too, as a
journal entry.

## The candidate entry format

```
Lesson: <one imperative sentence, true beyond the moment it happened>
Evidence: <the record, handoff, log entry or tool result, with its numbers>
Scope: role | technology | project
Role: <role slug>
Promote: yes | no — <why>
```

- **Lesson** changes what someone does next time. A fact a record already
  holds is not a lesson.
- **Evidence** names where it happened, with the number. A lesson without
  evidence is an opinion: drop it.
- **Scope** and **Role** repeat the call's parameters so the entry reads on
  its own.
- **Promote** is `yes` only for a `role` lesson that is general and not
  already in that role's global memory. `technology` entries are `no` until the
  brain exists; `project` entries are `no`, because they already sit where they
  are read.

## At `learn` — distilling the job

1. **Read** with `flow_status(project_dir, read)`: `log`,
   `lessons-candidates.md`, every `handoffs/<NN>-<role>.md` that `handoffs`
   lists, and `memory/<role>.md` for each role that worked in the job. The
   brief pastes the global tier.
2. **Distill** one candidate per root cause from every FIX round, BLOCKED
   handoff, refused tool call, deferred finding, and decision later reversed.
   Merge duplicates; drop what either memory tier already holds and what a
   record already carries.
3. **Record** each lesson with the call above, triaged by the rule above.
4. **Playbook note.** When the job had at most one FIX round in total (the sum
   of `flow_status`'s `fix_rounds`), record one more entry, `role`
   `orchestrator` and `scope` `role`, whose Lesson begins `Playbook:` — the
   lane, the phases run, what each brief carried that let its phase pass the
   first time, and the decisions that held. It is `Promote: no`: a playbook is
   longer than a memory line, and it waits in the queue for the user's review.
5. **Return the promote list**: every `Promote: yes` entry with its role and
   Lesson line, for the session to show the user.
6. **Close the job**: `learn` supplies no record, and the job ends with
   `flow_advance(project_dir, job_id, to_phase)` where `to_phase` is `closed`.

## Promotion — the orchestrating session's step

- Only at `learn`, only `role` candidates marked `Promote: yes`, and only after
  the user has seen the list.
- One lesson is one bullet in `~/.konnect/agents/<role>/MEMORY.md`, written
  with the session's own editor; a long lesson wraps, it never splits.
- **The 60-line cap.** After the edit, count the file's body lines. Over 60,
  compress in place before finishing — merge near-duplicates first, then drop
  the weakest line. Never leave the file over the cap.
- The curator never writes a `MEMORY.md` itself. It has no file-write tool,
  and it should not get one.

## Not a brain

No brain path is ever read or written — not by the curator, not by the
session. `technology` lessons stay queued in `records/lessons-candidates.md`
until the brain exists; nothing looks for, creates, or writes a brain
directory in the meantime.

## Rules

- Record your own mistake the moment you see it, with its evidence.
- One lesson, one destination; never the same lesson in two tiers.
- Never put a secret, a credential, or personal data in a lesson.
