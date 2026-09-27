#define StageRoot GetEnv("SHARBCUT_STAGE")
#define ReleaseRoot GetEnv("SHARBCUT_RELEASE")

[Setup]
AppId=SharbCut
AppName=SharbCut
AppVersion=0.2.1
AppPublisher=SharbCut
AppPublisherURL=https://github.com/sharbvane/sharbcut
DefaultDirName={localappdata}\Programs\SharbCut
DefaultGroupName=SharbCut
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#ReleaseRoot}
OutputBaseFilename=SharbCutSetup
Compression=lzma2/fast
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\SharbCut.exe
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
#if FileExists(CompilerPath + "Languages\ChineseSimplified.isl")
Name: "chinesesimp"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
#endif

[Files]
Source: "{#StageRoot}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\SharbCut"; Filename: "{app}\SharbCut.exe"
Name: "{autodesktop}\SharbCut"; Filename: "{app}\SharbCut.exe"; Tasks: desktopicon

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Run]
Filename: "{app}\SharbCut.exe"; Description: "{cm:LaunchProgram,SharbCut}"; Flags: nowait postinstall skipifsilent
