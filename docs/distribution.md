# Distribution Notes

## Build Targets

- `npm run dist` builds both `nsis` and `portable` Windows packages.
- `npm run dist:nsis` creates the installer only.
- `npm run dist:portable` creates the portable executable only.
- `npm run dist:dir` creates an unpacked app for smoke testing.
- `npm run release:serve` hosts the `dist/` folder over HTTP so a packaged app can test updates end-to-end.

## Release Assets

The generated artifacts land in `dist/`. For NSIS releases, keep these files together on your update host:

- `latest.yml`
- the generated `*-Setup-*.exe`
- the matching `.blockmap`

Portable releases are useful for quick internal sharing, but the NSIS package is the better base for auto-updates.

## Auto-Update Setup

Runtime update checks are enabled when the packaged app includes `build/update-config.json`. The `dist:*` scripts generate that file from your current environment automatically.

Example PowerShell packaging flow:

```powershell
$env:VSMONITOR_UPDATE_URL = "https://your-server.example.com/releases/windows"
$env:VSMONITOR_UPDATE_CHANNEL = "latest"
npm run dist:nsis
```

Recommended host layout:

```text
https://your-server.example.com/releases/windows/latest.yml
https://your-server.example.com/releases/windows/VS-Monitor IDE-Setup-2.1.0.exe
https://your-server.example.com/releases/windows/VS-Monitor IDE-Setup-2.1.0.exe.blockmap
```

## End-To-End Smoke Test

1. Build the installer with `VSMONITOR_UPDATE_URL` set.
2. Upload the generated installer, `latest.yml`, and `.blockmap` to your update host, or run `npm run release:serve` locally.
3. Install the packaged app from the generated NSIS installer.
4. Launch the installed app and use `Check for Updates` from the command palette.
5. Verify the Diagnostics screen reports the expected update source and that new releases download/install correctly.

## Packaging Checklist

1. Run `npm run check`.
2. Build the target package with one of the `dist:*` scripts.
3. Upload the NSIS installer, `latest.yml`, and `.blockmap` files to the update host.
4. Install the packaged build and verify `Check for Updates` from the command palette.
