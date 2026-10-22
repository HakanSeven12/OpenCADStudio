# Local codec integration: current recipe and historical snapshots

These files are historical checkpoint artifacts. For the consolidated application, use [the fork build guide](../../docs/fork-appimage-testing.md); it resolves the codec through committed Git dependencies without a local override. Use [the historical checkpoint guide](../../docs/checkpoints/dwg2010-codec-integration.md) only to reproduce its pinned source identities. Do not execute the Python snapshots for the current application.

| File | Purpose |
|---|---|
| `codec-local.toml.in` | Historical checkpoint source override template. Replace `__LOCAL_CODEC_PATH__` with the absolute restored codec path. |
| `Cargo.lock.integration.patch` | Historical temporary lock delta for app `ea4537429` and codec base `ff75fdd`: removes only acadrust's Git source. Apply only in the isolated build worktree, never to the root lockfile. |
| `prepare.py` | Historical unfinished provenance harness for app `0b414389c`; assumes an obsolete root HEAD and uses all-platform offline metadata. |
| `verify.py` | Historical seven-private-fixture stdio harness; never executed for the current build and does not prove strict-open or zero-recovery status. |

The old application is incompatible with the newer codec's plugin type registry. The guide replaces its preparation/build recipe with native-target `cargo tree`, `cargo pkgid`, focused public tests and a compatible pinned application worktree.

Private inputs, raw outputs, builds and new memory/task logs are not included in this checkpoint update.
