# Publishing to KiCad's Plugin and Content Manager

KopperDigger reaches KiCad users through two PCM channels. Both are fed by the
same release artifacts.

| Channel | URL users add | Who publishes |
|---|---|---|
| Self-hosted repository (GitHub Pages) | `https://the78mole.github.io/KopperDigger/repository.json` | `pages.yml`, automatically after every tagged release |
| Official KiCad repository (listed by default) | none | merge request to [kicad/addons/metadata](https://gitlab.com/kicad/addons/metadata), reviewed by the KiCad team |

## Versions and tags

Versions come from the commit messages ([Conventional Commits](https://www.conventionalcommits.org/)).
On every push to `main`, `version.yml` runs
[paulhatch/semantic-version](https://github.com/PaulHatch/semantic-version)
over the commits since the last stable tag `vX.Y.Z`:

| Commit | Next version |
|---|---|
| `fix: ...`, `fix(pcm): ...` | patch, 0.14.0 → 0.14.1 |
| `feat: ...` | minor, 0.14.0 → 0.15.0 |
| `feat!: ...`, or `BREAKING CHANGE:` in the body | major, 0.14.0 → 1.0.0 |
| `docs:`, `ci:`, `chore:`, `refactor:`, `test:`, ... | no release |

Only when a `feat` or `fix` (or breaking) commit came since the last stable
tag does the workflow tag the commit, run `release.yml` for that tag and then
republish the PCM repository (`pages.yml`). With squash merges the PR title
becomes the commit message, so PR titles need the same format. Renovate's
`chore(deps): ...` updates never release on their own.

`Cargo.toml` is not bumped by a commit: the release jobs stamp the tag's
version into the manifests before building (`packaging/set-version.py`).

By hand: *Actions → Version → Run workflow* with a version releases exactly
that version (`0.14.0`, `0.15.0-rc.1`). Automatic releases need a first stable
tag to count from; pre-release tags (`-rc.1`) are not counted. Pushing a tag
`v*` yourself still works as before.

## What a release produces

A release tag `vX.Y.Z` (or `vX.Y.Z-rc.1` for a pre-release) runs `release.yml`:

1. `pcm-package` builds one PCM zip per platform
   (`kidig-mcp-pcm-vX.Y.Z-{windows,macos,linux}.zip`) for *Install from File*.
2. `pcm-universal` merges them with `packaging/build-pcm-universal.py` into
   `kidig-mcp-pcm-vX.Y.Z.zip`, which carries every platform's binaries under
   `plugins/bin/<platform>/`, plus `kidig-mcp-pcm-vX.Y.Z.zip.pcm-version.json`
   (the version entry with `download_url`, `download_sha256`, `download_size`,
   `install_size`).
   One zip per version is required: KiCad's official repository rejects
   duplicate version numbers, and the PCM client looks versions up by string.
3. `release` attaches all of it to the GitHub release. Tags with `-` become
   pre-releases. KiCad's PCM accepts only digits in a version, so
   `v0.14.0-rc.1` is packaged as `0.14.0` with status `testing`; the stable
   `v0.14.0` replaces it in the self-hosted repository, and pre-releases are
   never submitted to KiCad's.
4. `pages.yml` (called by the Version workflow, or started by a Release for a
   hand-pushed tag via `workflow_run`; either way it runs on `main` and needs
   no tag rule on the `github-pages` environment) collects the `.pcm-version.json` of every release, runs
   `packaging/build-pcm-repo.py` and deploys `repository.json`,
   `packages.json`, `resources.zip` and `index.html` to GitHub Pages. The same
   site contains `kicad-addons/com.github.the78mole.kidig-mcp/metadata.json`
   and `icon.png`: the files for the official repository.

One-time setup: **Settings → Pages → Build and deployment → Source: GitHub
Actions**.

A dry run without a release: *Actions → Release → Run workflow* with a version
such as `0.0.0`. The packages appear as workflow artifacts.

## Submitting to the official KiCad repository

Requirements checked against <https://dev-docs.kicad.org/en/addons/>:

- Identifier `com.github.the78mole.kidig-mcp` is reverse-DNS on the hosting
  service and unique. Konnect was never listed there, so the fork rule does not
  apply.
- AGPL-3.0 is GPL-compatible. The v1 schema has no AGPL value, so `license` is
  `open-source` and the license is named in `description_full`.
- Metadata in English, source and issue tracker on GitHub.
- Packages that connect to **commercial services** (component lookup counts)
  must contact `plugins@kicad.org` first. KopperDigger queries LCSC/JLCPCB part
  data, so send that mail before or together with the merge request.

### Automated: workflow "Submit to KiCad addon repository"

`.github/workflows/kicad-addons-submit.yml` is started by hand (*Actions → Submit
to KiCad addon repository → Run workflow*). It collects the `.pcm-version.json`
of the published releases (pre-releases only if asked), merges them into the
`metadata.json` KiCad already lists (existing versions stay untouched, new ones
are appended), runs KiCad's own `ci/validate-package.py` and
`ci/validate-image.py`, pushes branch `kopperdigger-<version>` to your GitLab
fork and opens the merge request. Running it again for the same version updates
that branch and the open merge request; if KiCad already lists every release it
does nothing.

One-time setup:

1. GitLab: fork <https://gitlab.com/kicad/addons/metadata> into your account.
2. GitLab: *Preferences → Access tokens → Add new token*, scopes `api` and
   `write_repository`, with an expiry date you will notice.
3. GitHub: *Settings → Environments → New environment* `kicad-addons`
   (optionally with yourself as required reviewer, so every submission waits
   for a click), then in that environment
   - secret `KICAD_GITLAB_TOKEN`: the token from step 2
   - variable `KICAD_GITLAB_FORK`: the fork's path, e.g. `the78mole/metadata`

The KiCad team reviews and merges the merge request; nothing is listed before
that.

### By hand (needs a GitLab account)

1. Fork <https://gitlab.com/kicad/addons/metadata> on GitLab and clone it.
2. `git switch -c add-kopperdigger`
3. Copy `kicad-addons/com.github.the78mole.kidig-mcp/` from the Pages site
   (or the `pages.yml` run's artifact) to `packages/`.
4. Optional local check, from the metadata checkout:
   `python -m pip install -r ci/requirements.txt` then
   `python ci/validate-package.py --schema-v1 schema-v1.json --schema-v2 schema-v2.json com.github.the78mole.kidig-mcp packages/com.github.the78mole.kidig-mcp/metadata.json`
5. Push the branch. The fork's pipeline validates it and links a temporary PCM
   repository with just this package; test it in KiCad.
6. Open the merge request against `kicad/addons/metadata` `main`.

For every later release, run the workflow again (or repeat by hand with the
regenerated `metadata.json`; versions KiCad already lists must stay unchanged).
