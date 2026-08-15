# Boot-mode state table — for review before any code lands

Removing the legacy screens is a **state-machine change, not asset removal**: `Tour`,
`TokenTour`, `FactoryTest` and `StandAloneTest` are boot targets, so deleting them leaves
`config.rs:172-195` with arms that have nowhere to go. This table names the S-CAM behaviour
of every cell first.

## How `AttachState` is derived today (`config.rs:133-152`)

| badge attached? | stored type | `k0` | → AttachState |
|---|---|---|---|
| no | none | zeroed / wrong length | **FactoryNew** |
| no | none | valid 32 bytes | **TestedStandAlone** |
| no | set | — | **Unattached** |
| yes | matches stored | — | **Matched** |
| yes | differs | — | **Mismatched** |
| — | — | — | **FirstMate** (set during first mating, bypasses the above) |

**`k0` is the sole discriminator between FactoryNew and TestedStandAlone.** It is now
permanently zeroed and unrecoverable, so a detached, never-mated module can only ever report
FactoryNew. Removing `k0` handling therefore deletes that distinction and must be designed,
not deleted.

## Current routing vs proposed

| AttachState | today, `!is_developer` | today, `is_developer` | **S-CAM proposal** |
|---|---|---|---|
| FirstMate | `Idle` | `Idle` | `Idle` |
| FactoryNew | `FactoryTest` | `IdleDevMode` | **`Idle`** — FactoryTest is cut |
| TestedStandAlone | `Idle` | `Idle` | `Idle` |
| Matched | `Idle` / `Tour` | `IdleDevMode` / `Tour` | **`Idle`** — Tour is cut |
| Mismatched | `Idle` / `Tour` | `IdleDevMode` / `Tour` | **`Idle`** — Tour is cut |
| Unattached | `Password` / `TokenTour` | same | **`Idle`** — see below |

### Decision 1 — collapse `IdleDevMode` into `Idle`

Recommended. The two are already near-identical, and the differences all disappear:

| | `Idle` | `IdleDevMode` |
|---|---|---|
| power behaviour (`config.rs:269-270`) | `(true, SHORT_TIMEOUT)` | **identical** |
| redraw path (`ux.rs:840`) | shared arm | **identical** |
| key handling | real handler at `ux.rs:1815` | **`=> Some(k)` — no handling at all** (`ux.rs:1844`) |
| screen | logo | logo + `"DEV MODE"` label |

Two consequences worth stating plainly:

- **The idle buttons are inert on this badge today.** It is permanently in `IdleDevMode`,
  whose key arm passes every key through unhandled. Any S-CAM idle-button work must cover
  this state or it will do nothing on the actual hardware.
- `Idle`'s only real binding is `'🔥' → GeneScan`, which is being cut with gene exchange.
  So there is no behaviour left to preserve on either side, and the S-CAM handler replaces
  both.

Removing the `"DEV MODE"` label was requested. Note it currently indicates a real and
permanent condition, so after removal nothing on screen distinguishes a developer-mode badge.

### Decision 2 — `Unattached` boots to the idle screen, not `Password`

This is the significant behavioural change and it needs your agreement.

`Unattached` is the **detached module** — no badge carrier, previously mated. It is the
configuration the S-CAM feature set mostly targets, and today it never reaches a logo idle
screen at all: it boots straight to `VaultMode::Password`, a list view.

The todo's item 7 describes an idle screen with a logo and three button bindings. That is
incompatible with booting to a password list. The proposal routes `Unattached` to `Idle` so
the described behaviour exists in the configuration it was designed for; passwords remain
reachable from the menu.

### Decision 3 — `FactoryNew` needs a target regardless

`FactoryNew` is not a factory-only state. Three live routes reach it: zeroed `k0` (permanent
here), `dc34-console/src/cmds/bio.rs:107` calling `delete_dict(DC34_DICT, None)`, and the
`factory-new` cargo feature. Cutting `FactoryTest` without adding an arm leaves the
production path with no target — which would not bite on this badge, because developer mode
routes it to `IdleDevMode`, and would bite on any other.

## What this means for `k0`

After the cut, `k0` is read at `config.rs:87` and used only to pick between `FactoryNew` and
`TestedStandAlone` — and both now route to `Idle`. The distinction becomes unobservable, so
the read can go, and with it the last dependency on the erased key.

`k0_hash()` (`config.rs:237`) needs checking before removal; it is unrelated to routing.

## Open questions

1. Collapse `IdleDevMode` into `Idle`? (recommended)
2. Route `Unattached` to `Idle` rather than `Password`? (needed for item 7 to mean anything)
3. Anything that should still distinguish a developer-mode badge on screen, given the label
   is being removed and the state is permanent?
