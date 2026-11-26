# Code signing the exe

`larma-mic.exe` is not signed, so Windows SmartScreen shows "Windows protected your PC" on first run. Signing needs a
certificate that belongs to *you*, so it cannot be done from the repository alone. Two realistic options for an open source project:

## Option A: SignPath Foundation (free for open source)
1. Apply at <https://signpath.org/foundation> with the repository link (the project is MIT licensed, which they require).
2. When accepted, SignPath gives you an organization, a project and an API token.
3. Add the token as the repository secret `SIGNPATH_API_TOKEN`, and the ids as `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`
   and `SIGNPATH_SIGNING_POLICY_SLUG` (repository secrets or variables).
4. Add a step to the `release` job of `.github/workflows/build.yml` that uploads `larma-mic.exe` as an artifact and calls
   `signpath/github-action-submit-signing-request@v1`, then attach the signed file instead of the unsigned one.
   (SignPath documents the exact inputs; the steps above are what it needs from this repository.)

## Option B: a certificate you buy
Any Authenticode (OV or EV) certificate works. Sign with `signtool sign /fd SHA256 /tr <timestamp-url> /td SHA256 larma-mic.exe`
after the build. EV certificates remove the SmartScreen warning immediately; OV ones earn reputation over time.

SmartScreen reputation also builds up with downloads, so the warning fades even for unsigned files after many users run them.
