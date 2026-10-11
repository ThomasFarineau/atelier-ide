# Versions & releases

Versions are managed by [Knope](https://knope.tech) (`knope.toml`), as with Changesets: the files of `.changeset/` decide the version and the changelog, not the commit messages.

## 1. In the pull request

A change worth a line in the changelog comes with a change file, made by `knope document-change` (type, then summary) or written by hand in `.changeset/<summary>.md`:

```md
---
default: minor
---

# The terminal shows the memory and CPU use of each tab

Optional details, in Markdown.
```

`major` for a breaking change, `minor` for a feature, `patch` for a fix. While the version is `0.x`, `major` gives `0.(x+1).0`, and `minor` and `patch` both bump the last number.

## 2. On `main`

The **Prepare release** workflow (`.github/workflows/prepare-release.yml`) opens or updates the “chore: prepare release X.Y.Z” pull request from the `release` branch. It changes the version in `Cargo.toml` and `Cargo.lock`, adds the section of `CHANGELOG.md` and deletes the change files it used.

The workflow then builds the `release` branch: the files can be tried from the artifacts of the run, and the compiled dependencies stay in the cache for the release.

::: warning
That pull request is pushed by GitHub Actions, so the CI does not run on it: an admin merges it without waiting for the required checks. It only changes the version and the changelog.
:::

## 3. When that pull request is merged

The **Release** workflow (`.github/workflows/release.yml`) builds the IDE and its launcher for Windows, Linux and macOS (Intel and Apple Silicon) with `.github/workflows/build.yml`, along with the Windows installer. Then `knope release` creates the `vX.Y.Z` tag and publishes the GitHub release with the notes of the changelog. Thanks to the cache, only the code of Atelier is rebuilt. The release stays a draft until every file is uploaded, so the launcher never sees an incomplete one.

## Files of a release

| File | Use |
| --- | --- |
| `atelier-setup-windows-x86_64.exe` | The Windows installer |
| `atelier-<os>-<arch>` | The launcher, downloaded once on Linux and macOS |
| `atelier-ide-<os>-<arch>.zip` / `.tar.gz` | The archive the launcher installs: the IDE and the next launcher |
| `SHA256SUMS` | The checksums the launcher verifies |

The **Build** workflow can also be started by hand, to build without publishing. The repository must allow “Allow GitHub Actions to create and approve pull requests” (Settings › Actions › General) for the release pull request to be created.
