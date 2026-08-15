# Dead paths — edits here have no effect

Three separate traps in this tree silently discard your work: the file compiles, the build
succeeds, and nothing changes. Two of them cost real debugging time before they were found.
Check this list before editing anything unfamiliar.

Regenerate with the audit script at the bottom.

## 1. Modules that are never declared (1,202 lines, not compiled)

| file | lines |
|---|---|
| `src/ux/framework.rs` | 858 |
| `src/prereqs.rs` | 243 |
| `src/ux/icontray.rs` | 101 |

No `mod` declaration anywhere in the crate, so `rustc` never sees them. Editing them compiles
clean and changes nothing. Deleting them reclaims **zero bytes** — they were never in the
binary — but removes the footgun.

## 2. Dependencies not overridden by the `[patch]` table (7)

`bao1x-emu` · `locales` · `persistent_store` · `susres` · `userprefs` · `usb-bao1x` ·
`xous-usb-hid`

These resolve to `~/.cargo/git`, **not** the sibling `../xous-core` worktree. Editing the
local checkout of any of them has no effect on this build. 11 other crates *are* patched to
local paths, which makes the inconsistency easy to miss.

Known casualty: a previous session instrumented `usb-bao1x`'s `SendString` for HID
verification on branch `worktree-agent-a89fdbd521c069b88`. That work was never compiled.

## 3. `locales/i18n.json` in this repo is never read

`xous-core/locales/build.rs:147` derives `project_root()` from the **locales crate's own**
manifest directory's parent, then globs `{root}/**/i18n.json`. As a git dependency that root
is inside `~/.cargo/git`, so the build reads `apps-baosec/vault2/locales/i18n.json` and never
this repo's copy.

**Do not "fix" this by patching or vendoring.** Both were tried and both fail:

- *Patching to `../xous-core/locales`* leaves `project_root()` at `xous-core/`, so this
  repo's file stays unread. It fixes nothing.
- *Vendoring the crate into this repo* fails twice over: Cargo rejects the duplicate
  (`modals`, `pddb`, `ux-api`, `keystore` and `usb-bao1x` all already depend on `locales`),
  and patching it globally would move `project_root()` here and lose 45 `pddb`, 8 `ux-api`
  and 2 `modals` keys that live in xous-core.

This is a footgun, not a live bug: all 79 keys the code actually uses resolve correctly from
xous-core's copy today. New S-CAM strings use plain Rust constants instead — see
`DECISIONS.md`.

## Audit script

```python
import os, re
decls = set()
for root, _, files in os.walk('src'):
    for f in files:
        if f.endswith('.rs'):
            for m in re.finditer(r'^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z0-9_]+)\s*;',
                                 open(os.path.join(root, f), errors='ignore').read(), re.M):
                decls.add(m.group(1))
for root, _, files in os.walk('src'):
    for f in files:
        if not f.endswith('.rs'):
            continue
        p = os.path.join(root, f)
        if p in ('src/main.rs', 'src/lib.rs'):
            continue
        name = os.path.basename(root) if f == 'mod.rs' else f[:-3]
        if name not in decls:
            print('orphan module:', p)

t = open('Cargo.toml').read()
head = re.split(r'^\[patch\.', t, flags=re.M)[0]
body = ''.join(re.split(r'^\[patch\.', t, flags=re.M)[1:])
git = {m.group(1) for m in re.finditer(r'^([A-Za-z0-9_-]+)\s*=\s*\{[^\n]*\bgit\s*=', head, re.M)}
pat = {m.group(1) for m in re.finditer(r'^([A-Za-z0-9_-]+)\s*=\s*\{[^\n]*\bpath\s*=', body, re.M)}
for d in sorted(git - pat):
    print('unpatched git dep:', d)
```

Note the naive versions of both checks give wrong answers: matching `mod` against a bare
filename flags every `mod.rs` as an orphan, and a loose patch-table regex reports every
dependency as unpatched.
