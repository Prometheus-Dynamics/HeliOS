# Gaia builds in CI

`.github/workflows/gaia-builds.yml` runs HeliOS's Gaia builds on GitHub Actions.

| when | runner | what |
|---|---|---|
| push to `dev`, pull request (changes under `backend/`, `gaia/`, `tools/build-os.sh`, `rust-toolchain.toml`) | GitHub-hosted `ubuntu-24.04` | `gaia validate` of `raze.toml` and `systemcore.toml`; SystemCore's Rust artifacts cross-built with Limelight's SDK (`--only artifacts`, `dev-image` profile, no package) |
| Actions → Gaia builds → Run workflow | self-hosted, labels `self-hosted, linux, x64, gaia` | the full Raze image (`tools/build-os.sh raze full`) and the SystemCore ipk, uploaded as the run's artifact `helios-images-<n>` (kept 14 days) |

There is no nightly build and no build of images on push.

## The Eidos token (`EIDOS_READ_TOKEN`)

Eidos is a private repository, and the SystemCore artifacts depend on it. The
per-push artifact job needs a token that can read it.

1. Create a fine-grained personal access token at GitHub → Settings → Developer
   settings → Fine-grained tokens:
   - Resource owner: `Prometheus-Dynamics`
   - Repository access: only `Prometheus-Dynamics/Eidos`
   - Permissions: Contents: Read-only
   - An expiry you will remember to renew.
2. Add it to HeliOS as a repository secret named `EIDOS_READ_TOKEN`
   (HeliOS → Settings → Secrets and variables → Actions → New repository secret), or:

   ```bash
   gh secret set EIDOS_READ_TOKEN --repo Prometheus-Dynamics/HeliOS
   ```

Without the secret, or on a pull request from a fork (GitHub gives those no
secrets), the artifact job is skipped and the run shows a notice saying so.
Validation still runs: neither config needs Eidos to validate.

The token reaches git only, as an `insteadOf` rule for
`https://github.com/Prometheus-Dynamics/Eidos`, which Gaia's clone of the
`eidos` source uses. Cargo reads Eidos from Gaia's mirror of that clone. The
token is not in any build file, and GitHub masks it in logs.

## The self-hosted runner

The image job waits in the queue, harmlessly, until a runner with the `gaia`
label is online. To add one:

1. **Prepare the machine.** It needs everything a local HeliOS build needs:
   - Gaia, at the version HeliOS asks for:
     `cargo install --locked --git https://github.com/Prometheus-Dynamics/Gaia-Image-Builder gaia`
   - Rust 1.99.0 with the `aarch64-unknown-linux-gnu` and `aarch64-unknown-linux-musl` targets;
   - Docker (board-agent builds in it);
   - Buildroot's host packages;
   - `bun`;
   - about 100 GB of free disk.

   Run the runner as its own user, not your login user, and give that user the
   tools above.
2. **Get a registration token.** It needs admin rights on HeliOS. Either open
   HeliOS → Settings → Actions → Runners → New self-hosted runner (Linux, x64),
   or run:

   ```bash
   gh api -X POST repos/Prometheus-Dynamics/HeliOS/actions/runners/registration-token -q .token
   ```

   The token is valid for one hour.
3. **Install the runner** with the download and checksum commands that page
   shows (they name the current runner version). Then register it with the
   `gaia` label:

   ```bash
   ./config.sh --url https://github.com/Prometheus-Dynamics/HeliOS \
     --token <registration token> --name "$(hostname)-gaia" --labels gaia \
     --work _work --unattended
   sudo ./svc.sh install <runner user>
   sudo ./svc.sh start
   ```

   `self-hosted`, `linux` and `x64` are added automatically. `./svc.sh status`
   shows whether it is running. `sudo ./svc.sh stop` and `./config.sh remove
   --token <removal token>` take it off again.

### HeliOS is a public repository: lock the runner down first

On a public repository, a pull request from a fork runs workflow files from the
fork. A fork could add a job with `runs-on: [self-hosted, gaia]`, and without a
gate it would run on your machine. Before the runner goes online, set HeliOS →
Settings → Actions → General → the fork pull request approval setting to its
strictest option (approval required for all outside collaborators or external
contributors, not only first-time ones). Then no fork pull request runs anything until a maintainer
approves it. Check each such pull request's workflow changes before approving.
Running the runner as a dedicated user, with no access to your keys, limits
what a mistake can reach.

## Running the image build

Actions → Gaia builds → Run workflow, then pick the branch, `target` (`raze`,
`systemcore` or `both`) and `cargo_profile` (`release`, which ships, or
`dev-image`, which links faster). The images are the run's artifact. The
SystemCore step runs only on branches that have `gaia/configs/builds/systemcore.toml`.

The runner keeps Gaia's caches on its own disk: Buildroot's packages and
ccache, the downloads, and the cargo target dirs. A second run reuses
everything that did not change, as a local build does.
