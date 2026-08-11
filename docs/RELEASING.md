# Releasing

This repository ships two independent release lines:

| Line | Tag | Workflow | Output |
|---|---|---|---|
| Crate | `vX.Y.Z` | `.github/workflows/publish.yml` | `firecrawl-pdfium` on crates.io + a GitHub release |
| Native binaries | `native-vNNNN` | `.github/workflows/release-native.yml` | Repackaged PDFium archives as GitHub release assets, with attestations and SBOMs |

Every step of both pipelines is idempotent — the universal recovery move is
**"Re-run failed jobs"** (section 5).

---

## 1. One-time bootstrap: the first crates.io publish

> **Prerequisite:** complete section 2 first. The `release` environment and
> its required reviewers must exist *before* the first `v*` tag is pushed —
> a missing environment is created implicitly with **no protection rules**,
> which would leave the publish job ungated. (Before Trusted Publishing is
> configured the job still fails safely at token minting, but the gate
> should never be absent.)

crates.io Trusted Publishing **cannot create a new crate** — the first-ever
publish must use a classic API token. This is done once, by a maintainer,
locally; afterwards no long-lived credential exists anywhere.

Two variants; **A (placeholder) is what this repository used.** It keeps the
token entirely away from the real release: the token only ever publishes an
empty 0.0.0 name reservation, and 0.1.0 itself goes out through the gated
OIDC workflow like every future release.

### Variant A: 0.0.0 placeholder, then OIDC for the real release

1. Create a scoped token on <https://crates.io/settings/tokens>: *New
   Token* -> scope **`publish-new`** only, crate pattern `firecrawl-pdfium`,
   shortest available expiry.
2. Publish a minimal placeholder (name, `version = "0.0.0"`, description
   marking it a placeholder, `repository` pointing here, an empty lib.rs and
   a README saying "do not depend on 0.0.0"). Keep the token out of shell
   history and off disk:

   ```sh
   read -s CARGO_REGISTRY_TOKEN && export CARGO_REGISTRY_TOKEN
   cargo publish        # from the placeholder directory
   unset CARGO_REGISTRY_TOKEN
   ```

3. Configure Trusted Publishing (step 3 below), then **revoke the token**
   (step 4 below).
4. Cut the real release through the normal path (section 3): tag `v0.1.0`,
   approve the environment gate; the workflow publishes 0.1.0 via OIDC.
5. After 0.1.0 is live, **yank 0.0.0** from the crate's Versions page on
   crates.io (owners have a yank button) so nobody resolves the placeholder.

### Variant B: publish 0.1.0 directly with the token

1. **Create a scoped token.** On <https://crates.io/settings/tokens>:
   *New Token* → scope **`publish-new`** only, crate pattern
   `firecrawl-pdfium`, shortest available expiry. Do not store it in any
   password manager, CI secret, or shell profile — it lives for the next
   ten minutes.
2. **Publish 0.1.0 from a clean tagged checkout:**

   ```sh
   git clone https://github.com/firecrawl/pdfium-rs
   cd pdfium-rs
   git checkout v0.1.0           # tag must already exist and match Cargo.toml
   git status --porcelain        # must print nothing
   cargo publish --dry-run -p firecrawl-pdfium
   CARGO_REGISTRY_TOKEN=<token> cargo publish -p firecrawl-pdfium
   ```

   Note: pushing the `v0.1.0` tag also triggers `publish.yml`; its publish
   job is safe to let run — the sparse-index guard will see the version you
   just published manually and skip — or a reviewer can simply reject the
   environment approval.
3. **Configure Trusted Publishing.** On the crate's page →
   *Settings* → *Trusted Publishing* → *Add GitHub*:

   | Field | Value |
   |---|---|
   | Repository owner | `firecrawl` |
   | Repository name | `pdfium-rs` |
   | Workflow filename | `publish.yml` |
   | Environment | `release` |

   The environment field is what stops a workflow on a fork or an
   unapproved run from minting tokens.
4. **Revoke the token** on <https://crates.io/settings/tokens>, immediately.
   Verify the tokens list is empty. From now on, every publish is
   OIDC-only: `rust-lang/crates-io-auth-action` mints a 30-minute,
   auto-revoked token inside the approved workflow run.

## 2. One-time GitHub repository setup

1. **Create the `release` environment.** *Settings → Environments → New
   environment* → name `release`:
   - **Required reviewers**: add the release maintainers. This is the
     human approval gate — the `publish` job blocks until one of them
     approves the run.
   - **Deployment branches and tags** → *Selected branches and tags* → add
     tag pattern `v*`. Runs from any other ref cannot even request the
     environment.
2. **Actions settings.** *Settings → Actions → General*: Actions enabled;
   set *Workflow permissions* to **Read repository contents** (workflows in
   this repo declare their own per-job permissions and never rely on a
   permissive default).
3. Optional hardening: require the `release` environment's reviewer list to
   differ from the person who pushed the tag; enable tag protection for
   `v*` and `native-v*` so only maintainers can create release tags.

## 3. Normal crate release (`vX.Y.Z`)

1. **Prepare the release PR:**
   - Bump `version` in `Cargo.toml`.
   - Add a `## [X.Y.Z] - YYYY-MM-DD` section to `CHANGELOG.md`
     (`publish.yml` extracts this section into the GitHub release notes;
     if it is missing it falls back to auto-generated notes).
   - If the pinned PDFium release changed since the last crate release,
     update the compatibility table in `docs/VERSIONING.md`.
   - CI on the PR already exercises the packaging path
     (`publish-dry-run` job).
2. **Merge, then tag the merge commit:**

   ```sh
   git switch main && git pull
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

3. **Watch `Publish` run:**
   - `verify` re-checks tag == `Cargo.toml` version, runs the full test
     suite against the pinned PDFium build, and does a packaging dry run —
     all *before* anyone is asked to approve anything.
   - `publish` then waits on the **release environment gate**. Approve it
     in the run's UI (*Review deployments → release → Approve*).
   - `github-release` creates the GitHub release from the CHANGELOG
     section.
4. **Post-release verification:**
   - <https://crates.io/crates/firecrawl-pdfium> shows the new version.
   - docs.rs build is green (can lag by a few minutes):
     <https://docs.rs/firecrawl-pdfium>. The crate has no native link step,
     so docs.rs builds cannot fail for missing-PDFium reasons.
   - `gh release view vX.Y.Z --repo firecrawl/pdfium-rs` shows the release
     with notes.
   - Consumer smoke from scratch:

     ```sh
     cargo new /tmp/pdfium-smoke && cd /tmp/pdfium-smoke
     cargo add firecrawl-pdfium@X.Y.Z && cargo build
     ```

## 4. Native binaries release (`native-vNNNN`)

`NNNN` is the upstream chromium build number pinned in `pdfium.lock.json`
(e.g. `native-v7988` for `chromium/7988`), so the tag names the exact
upstream build it repackages.

### 4a. Re-pinning to a new upstream release

1. Pick the new release from
   <https://github.com/bblanchon/pdfium-binaries/releases> (say
   `chromium/8123`), then re-pin — this downloads every platform asset,
   records fresh sha256s (upstream publishes none), reads the archive's
   `VERSION`, and rewrites `pdfium.lock.json`:

   ```sh
   cargo xtask pin chromium/8123
   git diff pdfium.lock.json   # review: release, version, all five sha256s
   ```

2. Sanity-check locally — the fetch verifies every checksum, then the full
   test suite runs against the new binary:

   ```sh
   cargo xtask fetch-pdfium
   cargo test --all-targets
   ```

3. PR the lock change (CI runs the whole matrix against the new pin),
   merge.

### 4b. Cutting the release

```sh
git switch main && git pull
git tag native-v8123        # matches the chromium build just pinned
git push origin native-v8123
```

`Release native binaries` then, with no human gate (these are repackaged,
checksum-verified public artifacts, not a publish to a package registry):

1. `build`: `cargo xtask repackage --all` — re-downloads every upstream
   archive, verifies each sha256 against the lock, repackages with
   `PROVENANCE.json`, and writes `SHA256SUMS` + `ARTIFACT-SIZES.txt`;
   attests build provenance for every `.tgz`. Repackaged archives are
   **byte-reproducible** (in-process deterministic tar+gzip): the same
   lock file and tag always produce identical bytes, on any machine.
2. `sbom`: one SPDX SBOM per archive (syft). Note: syft over opaque shared
   libraries is thin by nature — the strong supply-chain links are the
   provenance attestation and the `PROVENANCE.json` inside each archive.
3. `release`: creates the GitHub release (notes generated from
   `pdfium.lock.json`) and uploads archives + `SHA256SUMS` + SBOMs.

### 4c. Verifying a native release

```sh
gh release download native-v8123 --repo firecrawl/pdfium-rs
shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify firecrawl-pdfium-linux-x64.tgz --repo firecrawl/pdfium-rs
tar xzf firecrawl-pdfium-linux-x64.tgz PROVENANCE.json && cat PROVENANCE.json
```

The attestation verify must report the workflow
`.github/workflows/release-native.yml` and the tag you released.

## 5. Failure and retry playbook

Design rule: **every step is idempotent**, so the first response to any red
job is *Actions → the failed run → Re-run failed jobs*. Specifics:

| Situation | What happened | What to do |
|---|---|---|
| `verify` fails on a `v*` tag | Nothing was published anywhere. | Fix on main. If the tag itself is fine (flaky test), re-run; if the tag pointed at broken code, delete the tag (`git push origin :refs/tags/vX.Y.Z`), and re-tag once fixed. Never reuse a version number that reached crates.io. |
| `publish` fails before `cargo publish` (gate rejected, token mint failed) | Nothing published. | Re-run failed jobs; approve the gate. |
| `cargo publish` itself fails midway | crates.io publishes atomically: either the version exists or it does not. | Re-run failed jobs. The sparse-index guard (`https://index.crates.io/fi/re/firecrawl-pdfium`) checks whether the version landed: if yes it skips the publish; if no it publishes again. Both paths are safe. |
| crates.io publish succeeded, `github-release` failed | The crate is live; only the GitHub release is missing. | Re-run **only** the failed `github-release` job. If the whole workflow is re-run instead, that is also safe: the guard skips the publish, `gh release view` skips an existing release. |
| The tag was cut from the wrong commit but nothing published yet | — | Delete and re-create the tag. Once a version is on crates.io it is immutable: never move a published tag; ship a patch release instead (and `cargo yank` the bad version if warranted). |
| `release-native` failed partway (e.g. after creating the release but before uploading all assets) | A release may exist with a subset of assets. | Re-run failed jobs. `gh release view \|\| gh release create` skips creation. The upload step is tamper-guarded: archives are byte-reproducible, so already-published assets are either identical (skipped) or **refused** if the bytes differ — a differing archive means the lock changed under the tag, or worse; investigate rather than override. Metadata assets (`SHA256SUMS`, sizes, SBOMs) refresh in place. Re-running `build` re-attests — additional attestations for identical subjects are harmless. |
| Upstream sha256 mismatch during `repackage` | Upstream asset changed or download corrupted; xtask refuses to continue. | Do not override. If upstream really replaced the asset, that is a re-pin (section 4a) with newly recorded checksums, reviewed via PR. |
| Environment approval request went to the wrong people / expired | Runs waiting on the gate can be approved for 30 days, then time out. | Re-run the workflow; adjust the reviewer list in the environment settings. |

## 6. Testing the pipeline without a production release

- **Packaging path**: exercised on every push by the `publish-dry-run` job
  in `ci.yml`, and locally with
  `cargo publish --dry-run -p firecrawl-pdfium`.
- **Publish workflow logic**: the `verify` job runs for any `v*` tag before
  the gate, so a tag on a throwaway branch exercises it; the reviewer then
  **rejects** the `release` environment approval and nothing publishes.
  Delete the tag afterwards. (Prefer the dry-run path above; use this only
  when validating changes to `publish.yml` itself.)
- **Native release, end to end, without a real release**:

  ```sh
  # The tag deliberately does NOT match the `native-v*` push filter —
  # a matching tag would fire the push trigger and cut a real, non-draft
  # release. Only the manual dispatch below runs for this tag.
  git tag test-native-v0 && git push origin test-native-v0
  gh workflow run release-native.yml -f tag=test-native-v0   # draft defaults to true
  ```

  `workflow_dispatch` runs create a **draft** release by default (the
  `draft` input), so nothing becomes publicly visible. Inspect the draft's
  assets/notes, verify the attestation, then clean up:

  ```sh
  gh release delete test-native-v0 --yes
  git push origin :refs/tags/test-native-v0
  ```

  Known quirk: `gh release view` may not find draft releases by tag on a
  re-run, so re-running a draft test can create a second draft — delete
  leftovers when done. Real (tag-push) runs are unaffected: those releases
  are non-draft and fully idempotent.
- **Repackaging locally** (writes under `target/`, which is gitignored):

  ```sh
  cargo xtask repackage --all --tag local-test --out target/dist
  tar tzf target/dist/firecrawl-pdfium-mac-arm64.tgz
  ```
