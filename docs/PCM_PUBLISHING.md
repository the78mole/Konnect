# Publishing to KiCad's Plugin and Content Manager

KopperDigger reaches KiCad users through two PCM channels. Both are fed by the
same release artifacts.

| Channel | URL users add | Who publishes |
|---|---|---|
| Self-hosted repository (GitHub Pages) | `https://the78mole.github.io/KopperDigger/repository.json` | `release.yml`, automatically on every tag |
| Official KiCad repository (listed by default) | none | merge request to [kicad/addons/metadata](https://gitlab.com/kicad/addons/metadata), reviewed by the KiCad team |

## What a release produces

Pushing a tag `vX.Y.Z` (or `vX.Y.Z-rc1` for a pre-release) runs `release.yml`:

1. `pcm-package` builds one PCM zip per platform
   (`kidig-mcp-pcm-vX.Y.Z-{windows,macos,linux}.zip`) for *Install from File*.
2. `pcm-universal` merges them with `packaging/build-pcm-universal.py` into
   `kidig-mcp-pcm-vX.Y.Z.zip`, which carries every platform's binaries under
   `plugins/bin/<platform>/`, plus `kidig-mcp-pcm-vX.Y.Z.zip.pcm-version.json`
   (the version entry with `download_url`, `download_sha256`, `download_size`,
   `install_size`).
   One zip per version is required: KiCad's official repository rejects
   duplicate version numbers, and the PCM client looks versions up by string.
3. `release` attaches all of it to the GitHub release (tags with `-` become
   pre-releases, and their PCM status is `testing`).
4. `pcm-repository` collects the `.pcm-version.json` of every release, runs
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

Steps (needs a GitLab account):

1. Fork <https://gitlab.com/kicad/addons/metadata> on GitLab and clone it.
2. `git switch -c add-kopperdigger`
3. Copy `kicad-addons/com.github.the78mole.kidig-mcp/` from the Pages site
   (or the `pcm-repository` job's artifact) to `packages/`.
4. Optional local check, from the metadata checkout:
   `python -m pip install -r ci/requirements.txt` then
   `python ci/validate-package.py --schema-v1 schema-v1.json --schema-v2 schema-v2.json com.github.the78mole.kidig-mcp packages/com.github.the78mole.kidig-mcp/metadata.json`
5. Push the branch. The fork's pipeline validates it and links a temporary PCM
   repository with just this package; test it in KiCad.
6. Open the merge request against `kicad/addons/metadata` `main`.

For every later release, repeat with the regenerated `metadata.json` (it holds
all versions; old entries must stay byte-identical, which the generator
guarantees because it rebuilds them from the published `.pcm-version.json`).
