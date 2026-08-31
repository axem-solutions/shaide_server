# Release steps

1. Create a release issue in Jira with "Release" set as the work type and the version to be released added for Fix versions
2. Create a new branch for the release issue
3. Set the new version number in the project's Cargo.toml
4. Create a new commit: "chore: release vX.Y.Z" and push it
5. Create a new empty PR titled 'RELEASE vX.Y.Z'
6. Merge the release branch to main
7. Switch to main and pull
8. Create a new tag on main (git tag -a vX.Y.Z -m "release name")
9. Push the tag (git push origin vX.Y.Z)
10. Create the new release on GitHub (with the appropriate release notes)
11. Fill out the remaining fields of the Jira release issue.
12. Release the Jira version

## Hotfix Release Process

Hotfixes are patch releases (vX.Y.Z+1) that address critical issues in production without including unreleased features from the main branch.

### When to Use a Hotfix

- Critical bugs in production
- Security vulnerabilities
- Data integrity issues
- Performance problems affecting users

### Hotfix Steps

1. Create a release issue in Jira with the patch version number
2. **Branch from the production tag**, for example:
   ```bash
   git checkout -b SHD-XXX/hotfix-v0.2.1 v0.2.0
   ```
3. Implement the minimal fix required
4. Update the version in Cargo.toml to the patch version
5. Create a release commit: "chore: release v{version number}"
6. Push the branch
7. Create and push the tag:
    ```bash
    git tag -a v{version number} -m "hotfix: [description]"
    git push origin v{version number}
    ```
8. Create the GitHub release with hotfix notes
9. Update and release the Jira version
