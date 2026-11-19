# bogovolution (the bogo command) on Windows

This zip has a Windows build of bogo (64 bit, x86_64), cross compiled
on Linux with mingw-w64. It needs no installer and no extra DLLs.

Contents:

- `bogo.exe`: the command line tool. The terminal view (`watch`) is
  built in: `bogo.exe watch`.
- `viewer\index.html`: the static lineage viewer, one file, works offline.
- `examples\demo\`: a small demo project and a recorded replay, for a free
  demo with no key and no network.
- `demo.ps1`: runs that demo.
- `LICENSE`, `CHANGELOG.md`, and this file.

## 1. Unzip and add to PATH

Unzip somewhere stable, for example `C:\Tools\bogo`. Then in
PowerShell:

```powershell
# this window only
$env:Path += ";C:\Tools\bogo"
# every new window (your user PATH; open a new window afterwards)
[Environment]::SetEnvironmentVariable("Path", [Environment]::GetEnvironmentVariable("Path", "User") + ";C:\Tools\bogo", "User")
bogo --version
```

## 2. Install sh (needed for check commands)

Every task's check command runs with `sh`, as on Linux. Without isolation
(fine for the free demo, which only runs the mock and replay providers) that
is the `sh.exe` on your PATH. Install Git for Windows and add its `usr\bin`
folder:

```powershell
$env:Path += ";C:\Program Files\Git\usr\bin"
sh -c "echo sh works"
```

Without it, a run stops with "could not start the check command (sh)" and
nothing is recorded.

## 3. Free replay demo (no key, no cost, no network)

From the unzipped folder:

```powershell
powershell -ExecutionPolicy Bypass -File .\demo.ps1
```

Or the same by hand:

```powershell
mkdir demo-run
Copy-Item -Recurse examples\demo\fixture\* demo-run
bogo -C demo-run init --model demo-model
Copy-Item examples\demo\tasks.toml, examples\demo\mock.toml demo-run\.bogo
bogo -C demo-run evolve --generations 10 --seed 7 --provider replay --replay-file "$PWD\examples\demo\replay.jsonl"
bogo -C demo-run status
bogo -C demo-run verify-log
bogo -C demo-run watch
```

Give `--replay-file` as a full path (as above): a relative one is read
from the project folder given with `-C`, not from the current folder.

Expected: "Accepted 2, rejected 5, noise 1, refused 2", "Current:
cd28eb296938 (was 8e28956afd69)", "Served 550 answers from the replay file",
and `verify-log` says "OK: 64 events". In `watch`, `q` quits.

Which isolation the demo uses depends on what works (`auto`):

- Docker Desktop running and `docker.io/library/alpine:3.20` pulled (see
  section 5): "Isolation: docker". Each check runs in a container, so the
  demo is slower (one container per check, 550 checks).
- Otherwise "Isolation: none (UNSAFE: no operating system isolation)", with
  Git for Windows' `sh`. That is fine for the demo: the replay provider only
  writes "ok" or "no" into answer files.

To pick one, add `--isolation none` or `--isolation docker` to the `evolve`
line (or set `isolation` under `[sandbox]` in `demo-run\.bogo\config.toml`).

## 4. Viewer

Open `viewer\index.html` in a browser. It shows the bundled demo lineage. To
see your own:

```powershell
bogo -C demo-run export --out demo-tree.json
```

then click **Open tree.json** (or drag the file onto the page). It runs
entirely in the browser, sends nothing anywhere, and loads nothing from the
network.

## 5. Isolation: Docker Desktop or WSL2

Real model runs (the `local` model server and the paid providers) are refused
without operating system isolation. bubblewrap, the Linux default, does not
exist on Windows. Two options:

- **Docker Desktop** (or Podman Desktop). Install it, start it, and pull the
  image once (runs never pull):

  ```powershell
  docker pull docker.io/library/alpine:3.20
  bogo -C demo-run isolation
  ```

  `isolation` (run inside a project, such as the demo above) should list
  docker as working. Checks then run in a container
  with no network, a read only root, and memory, CPU and process limits. The
  image needs whatever your checks use (alpine has only `sh`; for Python
  tasks, build or pull an image with Python and set `[sandbox] image` in
  `.bogo\config.toml`).
- **WSL2**. Install Ubuntu or Debian under WSL2, build or copy the Linux
  binary there, and run everything inside WSL2 with bubblewrap
  (`sudo apt install bubblewrap`). This is the setup the tool was tested on.

If no backend works, a real run stops with "no isolation backend works here"
and says which of these to install.

## 6. Claude or OpenAI on your own key (costs money)

**Every task attempt and every mutation is a billed call on your account.** A
default `evolve` with 8 tasks, 5 repeats and 3 generations makes up to 163
calls. Set a spending limit in your provider account first. bogo has
never been run against a paid endpoint by its author.

The key is read only from an environment variable, never from a file or a
flag:

```powershell
# this window only
$env:ANTHROPIC_API_KEY = "sk-ant-..."
# saved for new windows (your user environment; open a new window afterwards)
setx ANTHROPIC_API_KEY "sk-ant-..."
```

`setx` stores the key in your user registry in plain text; the `$env:` form
keeps it in that window only. Then, with Docker Desktop running:

```powershell
bogo -C my-repo evolve --generations 3 --provider anthropic --api-model <model id> --allow-spend --isolation docker
```

Nothing is sent unless `--allow-spend` and `--api-model` are on the command
line and the key variable is set. Add `--record run.jsonl` to replay the run
later for free. To remove the saved key, delete it under System Properties, Environment
Variables (or `setx ANTHROPIC_API_KEY ""`, which leaves it empty; an empty
key counts as not set).

## Windows caveats

- Built and tested on Linux, and under Wine 10 (busybox as `sh`, and a
  stand in `docker.exe` that records its arguments): the replay demo with
  and without docker, and the unit tests. The first run on a real Windows
  PC with Docker Desktop found that the drive letter's `:` was refused
  (fixed: docker and podman now get `--mount type=bind,...` on Windows).
  Real model runs on Windows are untested.
- Docker and podman: the project and the temp folder must be on a local
  drive (`C:\...`). Network shares (`\\server\share`) are refused, and so
  is a path with `,` `:` or `"` after the drive letter. Containers run as
  user `nobody` (65534); Docker Desktop's shares of Windows drives let any
  user write, so the check can write the worktree. Podman Desktop is
  untested.
- Line endings: a check script checked out with Windows line endings (CRLF)
  fails inside a Linux container (`sh: not found` or `\r` errors). Keep
  scripts LF, for example with a `.gitattributes` line `*.sh text eol=lf`.
- Check commands use `sh`, so tasks written for Linux need Git for Windows
  (or a container image that has what they need).
- A timed out check is stopped with `taskkill /T /F`.
- Without isolation, check commands get a few Windows system variables
  (`SystemRoot`, `TEMP`, `USERPROFILE` and similar paths) on top of `HOME`
  and `PATH`, because Windows programs fail without them.
- Line endings: keep rules files and `tasks.toml` as they are; git's
  `core.autocrlf` changing `AGENTS.md` to CRLF makes it a different version
  (a different hash) from one with LF.
