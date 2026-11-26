# Publishing to winget

The manifests live in `packaging/winget/template`. After a release (for example `v0.6.0`):

```powershell
cd packaging\winget
.\update.ps1 -Version 0.6.0            # downloads the released exe, fills the version, URL and SHA-256
wingetcreate submit out\0.6.0          # or open a pull request to microsoft/winget-pkgs with the files in out\0.6.0
```

`wingetcreate` is installed with `winget install wingetcreate`. Submitting opens a pull request from *your* GitHub account, so it
is done by you. Once merged, users install with `winget install mukii936.LarmaMic`.
