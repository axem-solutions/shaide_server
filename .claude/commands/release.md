---
disable-model-invocation: true
argument-hint: <version>
---

Release this component (`shaide_server`). Usage: `/release <version>`

Run this command from the component's own repo directory; it operates on the current git repository. VERSION is required in `vX.Y.Z` or `X.Y.Z` format.

---

## Steps to follow

Treat `$ARGUMENTS` as VERSION.

**1. Validate.**

- VERSION is required. Normalize to `vX.Y.Z`. Reject if not valid semver.
- Confirm the current repo is `shaide_server`: `git remote get-url origin` should end in `shaide_server` or `shaide_server.git`. If it does not, stop and warn that you are in the wrong directory.

**2. Pre-flight checks.**

Run these three checks before doing anything else:

```bash
# Must be on main
git rev-parse --abbrev-ref HEAD

# Working tree must be clean
git status --porcelain

# Tag must not already exist
git tag -l <VERSION>
```

Stop immediately with a clear message if any fails:
- Branch is not `main` → "Must be on main to release."
- Status output is non-empty → "Working tree is dirty. Commit or stash all changes first: `<list the dirty files>`"
- Tag output is non-empty → "Tag <VERSION> already exists."

**3. Generate CHANGELOG content.**

Get commits since the last tag:
```bash
git log \
  "$(git describe --tags --abbrev=0 2>/dev/null || \
     git rev-list --max-parents=0 HEAD)..HEAD" \
  --pretty=format:"%s" --no-merges
```

Categorize commits by conventional commit type into Keep a Changelog format:
- `feat:` / `feat(<scope>):` → **Added**
- `fix:` / `fix(<scope>):` → **Fixed**
- `chore:`, `refactor:`, `perf:`, `docs:`, `ci:`, `ops:` → **Changed**
- `feat!:` / `fix!:` or `BREAKING CHANGE` footer → **Added** with `**Breaking:**` prefix
- Merge commits and `chore(release):` → omit
- Commits without a conventional prefix (e.g. branch-name-style subjects): inspect the commit body and diff with `git show <hash>` and write a clear human-readable description; categorize as **Fixed** or **Changed** based on content.

Strip any trailing PR references (e.g. `(#123)`) from all changelog entries.

Format:
```
### Added
- <feat commit messages>

### Fixed
- <fix commit messages>

### Changed
- <other commit messages>
```
Omit empty sections. Use today's date for the header (the script adds the `## [X.Y.Z] - date` line).

Write to `/tmp/changelog_shaide_server.md`.

**4. Build the PR body.**

Jira release name: `shaide server <VERSION>`

Write `/tmp/release_pr_body_shaide_server.md`:

```markdown
## Release <VERSION>

**Jira Release:** shaide server <VERSION>

## Changes

<changelog content>
```

**5. Show the PR body and confirm.**

Print the PR body and say: "I'm about to create a release branch and open a PR. Proceed? (type 'yes' to confirm or provide corrections)"

Wait for confirmation. If the user provides corrections, update the temp files.

**6. Run the release script.**

```bash
npx tsx scripts/release.ts <VERSION> /tmp/changelog_shaide_server.md /tmp/release_pr_body_shaide_server.md
```

**7. Generate Jira Release notes for manual entry.**

Derive `<repo>` from the git remote URL. Derive `<prev-tag>` with:
```bash
git describe --tags --abbrev=0
```

Print the following block for the user to copy into the Jira Release "Release notes" field manually:

```
Goal of release: <one sentence summary derived from the CHANGELOG content for this version>
Previous version: <prev-tag>
Github commit changes link: https://github.com/axem-solutions/<repo>/compare/<prev-tag>...<VERSION>
Release notes link for the given version: https://github.com/axem-solutions/<repo>/releases/tag/<VERSION>
```

**8. Report result.**

Report the PR that was opened. Remind the developer:
- Merge the PR to trigger CI, which will create the tag and publish the release automatically.
- Paste the Jira release notes above into the `shaide server <VERSION>` release in the shaide Jira project.

---

## Example invocation

```
/release v1.2.0
```
