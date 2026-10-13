# Household Flatpak application

This application has a private GitHub repository in ThomasAndJuliet. Tom and
Juliet administer it equally. Keep application code here and deployment
infrastructure in ThomasAndJuliet/neo-infrastructure. The standalone repositories in Neo's Share/Projects are shared working
checkouts. Coordinate edits, preserve other admins' work and push changes to
the private household remote. Personal checkouts remain independent.

## Validation and releases

- Read this application's build instructions and `.github/workflows/` before
  editing. Run its documented test suite and packaging checks for your changes.
  Report the commands and their results. Do not replace substantive tests with
  smoke checks or silently disable failing tests to obtain a release.
- Every push to the default release branch (normally `main`) that passes its
  complete required CI suite must publish a Flatpak release automatically.
  No manual tag or release approval is required for ordinary private releases.
- Feature pushes and pull requests run CI and build checks without publishing
  to stable. Prefer pull requests into the release branch. Merge after the required checks and any required reviews pass.
  ThomasAndJuliet currently uses GitHub Free: private-repository PR merges are
  manual. Use native auto-merge only if a future plan enables it; never bypass
  the configured checks or reviews.
- Test the exact commit being released. Package that tested revision, retain
  the build artifact and record its Git SHA and CI run with the publication.
  Preserve each successful release build; cancellation or replacement of
  pending runs must not silently discard required releases. Publication must
  be ordered so an older, slower run cannot overwrite a newer stable release.
- CI must check the application's existing suite and validate the finished
  Flatpak build, AppStream metadata, icons and desktop launchable. Document
  the actual commands in README.md; resolve app-specific commands when adopting
  this template rather than adding invented generic checks.

## Publication and secrets

- All current household apps publish to the private `home-apps` repository.
  A source repository's privacy and a Flatpak branch name are independent of
  publication visibility. Neither `stable` nor `beta` authorizes public access.
- Public publication requires an explicit human decision and the separate
  public target, storage and signing identity. Never change visibility as a
  shortcut to resolve a CI networking or deployment failure.
- GitHub Actions submits built artifacts to Neo over narrowly scoped tailnet
  access. Signing happens on Neo in the publisher Quadlet. Never export the
  signing key into GitHub, a build runner, an app repository or chat.
- Production credentials stay outside Git. The infrastructure repository may
  contain whole-file Ansible Vault encrypted sources; its unlock password stays
  outside Git and is never needed for ordinary app CI. Prefer short-lived CI
  identity; never use either admin's personal login for unattended publication.
- Never run untrusted pull-request code with production credentials or publish
  from `pull_request_target`. Use read-only workflow permissions by default;
  grant deployment access only to the release job for the configured release
  branch and exact repository identity. Required-check branch protection is
  unavailable for private repositories on the current Free plan.

## Runtime and recovery

- Every HTTP content server or application backend on Neo runs in a Quadlet.
  Ansible owns host policy and configuration. Preserve enforcing SELinux,
  read-only mounts where possible, and service-scoped labels. Do not use
  privileged mode, disable labels or relabel shared storage to fix access.
- Preserve user data and other applications. Roll back a broken app by
  publishing a known-good build as a forward update, with its source revision
  recorded; check data compatibility before reverting code.
- Treat missing CI submission infrastructure as incomplete deployment. Do not
  claim that passing CI has published a release until the signed repository
  contains the expected app commit and the publication receipt confirms it.
