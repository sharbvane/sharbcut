# SharbCut

<div align="center">
  <img src="design-reference/sharbcut-logo.png" alt="SharbCut logo" width="112">
  <p><strong>A Windows-first desktop video editor with an editable timeline, AI-assisted editing, and beat-synced montage.</strong></p>
</div>

SharbCut is an AGPL-licensed desktop editor developed from [Concat](https://github.com/jub0t/Concat). Manual edits, AI edits, and beat-synced montages all work on the same project timeline; generated cuts remain editable and can be undone or redone.

## Features

- **Editable timeline:** multi-track clips, preview, transitions, effects, audio, titles, subtitles, and export.
- **AI Agent:** describe edits in natural language. SharbCut validates the proposed timeline commands before applying them as an undoable edit.
- **BGM montage:** create beat-synced edits from media already in the project and place source-backed clips directly on the timeline.
- **BMTS-lite by default:** the fast mode is used for ordinary montage requests. Full BGM-Montage analysis remains available as an advanced option.
- **Text and subtitles:** add and edit text clips without flattening them into a rendered video.
- **Undo and redo:** continue editing manually after AI or montage operations.
- **Windows packages:** installer and portable ZIP include the application runtime and media tools; end users do not need Rust, Cargo, CMake, or FFmpeg installed separately.

## Preview

The image is a saved UI concept from `design-reference`, not a screenshot of the current application build.

![SharbCut dark UI concept](design-reference/UI概念图/91d41443-5ded-4046-a06e-1b7965d9a235.png)

## Download and install

The first public release is [v0.3.0](https://github.com/sharbvane/sharbcut/releases/tag/v0.3.0).

- [Windows x64 installer — SharbCutSetup.exe](https://github.com/sharbvane/sharbcut/releases/download/v0.3.0/SharbCutSetup.exe): run the installer and launch SharbCut from the Start menu.
- [Portable package — SharbCut-Windows-x64.zip](https://github.com/sharbvane/sharbcut/releases/download/v0.3.0/SharbCut-Windows-x64.zip): extract the archive and run `SharbCut.exe`. The `portable` directory stores local settings and downloaded models beside the application.

Both packages are intended for Windows x64. Release notes include SHA-256 checksums. Projects reference source media at its existing location; moving or deleting those files can make timeline clips unavailable.

## Configure AI editing

Open **AI Editing** in SharbCut and enter an OpenAI Chat Completions-compatible endpoint, API key, and model. For example:

```text
Base URL: https://api.example.com/v1
Model: your-model-name
API key: enter your own key in the application
```

Use HTTPS for remote providers. Local HTTP is accepted for `localhost` and `127.0.0.1`. The API key is stored in Windows Credential Manager and is not written to the project file. Reasoning effort is optional and depends on the provider.

When AI editing is requested, SharbCut can send project and timeline metadata, media names and paths, up to four low-resolution preview frames, and—when available for selected media—an audio summary or short transcript to the configured endpoint. Review that provider's privacy terms before sending private footage or project data. Normal local editing and BGM montage do not require an AI service.

## Development

Development targets Windows x64 with the MSVC toolchain and Windows SDK. The setup script keeps project-manageable tools and caches in ignored `.tools` / `vendor` directories; Visual Studio Build Tools and the Windows SDK are installed normally.

```powershell
.\scripts\setup-dev.ps1
Push-Location .\engine
cargo check -p concat --locked
cargo build -p concat --locked
Pop-Location
.\scripts\run-dev.ps1 -SkipBuild
```

Create the Windows installer and portable ZIP with:

```powershell
.\scripts\package-windows.ps1
```

Daily montage testing should use BMTS-lite and a small set of real media. Run full BGM-Montage analysis only when testing its algorithm, full-mode compatibility, or a release regression.

## Project status

SharbCut is under active development. This is the first public Windows x64 release; the project is not yet declaring cross-platform release support. Issues and focused contributions are welcome.

## License and credits

SharbCut's Concat-derived source is licensed under **AGPL-3.0-or-later**. The original Concat license exceptions, contribution terms, and trademark notices remain in the repository; see [LICENSE](LICENSE), [LICENSE-EXCEPTIONS.md](LICENSE-EXCEPTIONS.md), [CLA.md](CLA.md), and [TRADEMARK.md](TRADEMARK.md).

BGM-Montage and BMTS-lite are separate projects with their own source-available, non-commercial licenses. They are not relicensed by this repository or covered by its root `LICENSE`. Their licenses and the compatibility patch attribution are documented in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The official SharbCut distribution is published with the component copyright holder's authorization; that does not grant general commercial-use rights to downstream users.

Other bundled or linked third-party components—including FFmpeg, Slint, Rust crates, fonts, and speech / vision libraries—are credited with their applicable terms in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Keep the included notices with redistributed builds.
